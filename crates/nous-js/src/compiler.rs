//! Compilador: AST do `oxc` → bytecode de registradores.
//!
//! Cada função tem sua janela de registradores. Parâmetros ocupam os primeiros, depois
//! vêm as variáveis locais (`var` sobe para o começo da função; `let`/`const` vivem no
//! bloco) e por cima ficam os temporários das expressões, alocados e liberados como uma
//! pilha. No topo do script, `var`, `let`, `const` e funções viram propriedades do
//! objeto global, e o registrador 0 guarda o valor da última expressão (o "completion
//! value" que o REPL mostra).

use std::collections::HashMap;
use std::rc::Rc;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_str::JSStr;

use crate::bytecode::{BinOp, FunctionCode, Op, Reg, UnOp};
use crate::heap::Heap;
use crate::value::Value;
use crate::vm::{JsError, number_to_string};

type CResult<T> = Result<T, JsError>;

fn unsupported<T>(what: impl Into<String>) -> CResult<T> {
    Err(JsError::Unsupported(what.into()))
}

/// Compila um script inteiro. O resultado roda com `this` = objeto global.
pub fn compile_script(heap: &mut Heap, source: &str) -> CResult<FunctionCode> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::script()).parse();
    if let Some(err) = ret.diagnostics.errors().next() {
        return Err(JsError::Syntax(err.to_string()));
    }
    if ret.fatal_error {
        return Err(JsError::Syntax("erro fatal de sintaxe".into()));
    }
    let mut c = Compiler {
        heap,
        fns: vec![FnState::new("<script>", 0, false, true)],
    };
    c.program(&ret.program)?;
    let state = c.fns.pop().expect("estado do script");
    Ok(state.finish())
}

// ------------------------------------------------------------------- estado

struct Scope {
    names: Vec<(String, Reg)>,
    saved_next: u16,
    saved_locals_top: u16,
}

struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    try_depth: usize,
    /// `switch` aceita `break` mas não `continue`.
    accepts_continue: bool,
}

struct FnState {
    name: String,
    params: u16,
    is_arrow: bool,
    is_script: bool,
    code: Vec<Op>,
    constants: Vec<Value>,
    const_map: HashMap<u64, u32>,
    functions: Vec<Rc<FunctionCode>>,
    scopes: Vec<Scope>,
    next_reg: u16,
    max_reg: u16,
    /// Nenhum temporário pode ser liberado abaixo disto (há variáveis locais vivas).
    locals_top: u16,
    loops: Vec<LoopCtx>,
    try_depth: usize,
}

impl FnState {
    fn new(name: &str, params: u16, is_arrow: bool, is_script: bool) -> Self {
        FnState {
            name: name.to_owned(),
            params,
            is_arrow,
            is_script,
            code: Vec::new(),
            constants: Vec::new(),
            const_map: HashMap::new(),
            functions: Vec::new(),
            scopes: vec![Scope {
                names: Vec::new(),
                saved_next: 0,
                saved_locals_top: 0,
            }],
            next_reg: 0,
            max_reg: 0,
            locals_top: 0,
            loops: Vec::new(),
            try_depth: 0,
        }
    }

    fn finish(self) -> FunctionCode {
        FunctionCode {
            name: self.name.into_boxed_str(),
            params: self.params,
            registers: self.max_reg.max(1),
            code: self.code,
            constants: self.constants,
            functions: self.functions,
            is_arrow: self.is_arrow,
        }
    }
}

enum Res {
    Local(Reg),
    Global,
    Captured,
}

struct Compiler<'h> {
    heap: &'h mut Heap,
    fns: Vec<FnState>,
}

fn js_string(s: JSStr<'_>) -> String {
    match s.as_str() {
        Some(s) => s.to_owned(),
        // contém surrogate solto: vira U+FFFD (as strings do heap são UTF-8)
        None => String::from_utf16_lossy(&s.encode_utf16().collect::<Vec<u16>>()),
    }
}

fn map_binary(op: BinaryOperator) -> BinOp {
    match op {
        BinaryOperator::Addition => BinOp::Add,
        BinaryOperator::Subtraction => BinOp::Sub,
        BinaryOperator::Multiplication => BinOp::Mul,
        BinaryOperator::Division => BinOp::Div,
        BinaryOperator::Remainder => BinOp::Rem,
        BinaryOperator::Exponential => BinOp::Exp,
        BinaryOperator::LessThan => BinOp::Lt,
        BinaryOperator::LessEqualThan => BinOp::Le,
        BinaryOperator::GreaterThan => BinOp::Gt,
        BinaryOperator::GreaterEqualThan => BinOp::Ge,
        BinaryOperator::Equality => BinOp::Eq,
        BinaryOperator::Inequality => BinOp::Ne,
        BinaryOperator::StrictEquality => BinOp::StrictEq,
        BinaryOperator::StrictInequality => BinOp::StrictNe,
        BinaryOperator::BitwiseAnd => BinOp::BitAnd,
        BinaryOperator::BitwiseOR => BinOp::BitOr,
        BinaryOperator::BitwiseXOR => BinOp::BitXor,
        BinaryOperator::ShiftLeft => BinOp::Shl,
        BinaryOperator::ShiftRight => BinOp::Shr,
        BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
        BinaryOperator::In => BinOp::In,
        BinaryOperator::Instanceof => BinOp::InstanceOf,
    }
}

/// Expressão sem efeitos colaterais: pode ser lida direto do registrador de origem.
fn is_simple(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::Identifier(_)
            | Expression::ThisExpression(_)
    )
}

// ------------------------------------------------- hoisting de `var`

fn binding_name<'a>(p: &BindingPattern<'a>) -> Option<&'a str> {
    match p {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn var_names<'a>(d: &VariableDeclaration<'a>, out: &mut Vec<&'a str>) {
    if d.kind == VariableDeclarationKind::Var {
        out.extend(
            d.declarations
                .iter()
                .filter_map(|decl| binding_name(&decl.id)),
        );
    }
}

/// Junta os nomes declarados com `var` (sem entrar em funções aninhadas).
fn collect_vars<'a>(s: &Statement<'a>, out: &mut Vec<&'a str>) {
    match s {
        Statement::VariableDeclaration(d) => var_names(d, out),
        Statement::BlockStatement(b) => b.body.iter().for_each(|s| collect_vars(s, out)),
        Statement::IfStatement(i) => {
            collect_vars(&i.consequent, out);
            if let Some(a) = &i.alternate {
                collect_vars(a, out);
            }
        }
        Statement::WhileStatement(w) => collect_vars(&w.body, out),
        Statement::DoWhileStatement(w) => collect_vars(&w.body, out),
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(d)) = &f.init {
                var_names(d, out);
            }
            collect_vars(&f.body, out);
        }
        Statement::ForInStatement(f) => {
            if let ForStatementLeft::VariableDeclaration(d) = &f.left {
                var_names(d, out);
            }
            collect_vars(&f.body, out);
        }
        Statement::ForOfStatement(f) => {
            if let ForStatementLeft::VariableDeclaration(d) = &f.left {
                var_names(d, out);
            }
            collect_vars(&f.body, out);
        }
        Statement::TryStatement(t) => {
            t.block.body.iter().for_each(|s| collect_vars(s, out));
            if let Some(h) = &t.handler {
                h.body.body.iter().for_each(|s| collect_vars(s, out));
            }
            if let Some(f) = &t.finalizer {
                f.body.iter().for_each(|s| collect_vars(s, out));
            }
        }
        Statement::LabeledStatement(l) => collect_vars(&l.body, out),
        Statement::SwitchStatement(sw) => {
            for case in &sw.cases {
                case.consequent.iter().for_each(|s| collect_vars(s, out));
            }
        }
        _ => {}
    }
}

impl Compiler<'_> {
    // --------------------------------------------------------- utilitários

    fn f(&mut self) -> &mut FnState {
        self.fns.last_mut().expect("função atual")
    }

    fn emit(&mut self, op: Op) -> usize {
        let f = self.f();
        f.code.push(op);
        f.code.len() - 1
    }

    fn here(&mut self) -> u32 {
        self.f().code.len() as u32
    }

    fn patch(&mut self, at: usize, to: u32) {
        match &mut self.f().code[at] {
            Op::Jump { target }
            | Op::JumpIfFalse { target, .. }
            | Op::JumpIfTrue { target, .. }
            | Op::JumpIfNotNullish { target, .. } => *target = to,
            Op::EnterTry { catch_target, .. } => *catch_target = to,
            other => unreachable!("patch em instrução que não é salto: {other:?}"),
        }
    }

    fn patch_here(&mut self, at: usize) {
        let h = self.here();
        self.patch(at, h);
    }

    fn alloc(&mut self) -> CResult<Reg> {
        let f = self.f();
        let r = f.next_reg;
        if r == Reg::MAX {
            return unsupported("função com registradores demais");
        }
        f.next_reg += 1;
        f.max_reg = f.max_reg.max(f.next_reg);
        Ok(r)
    }

    fn mark(&mut self) -> u16 {
        self.f().next_reg
    }

    fn release(&mut self, mark: u16) {
        let f = self.f();
        f.next_reg = mark.max(f.locals_top);
    }

    fn constant(&mut self, v: Value) -> u32 {
        let f = self.f();
        if let Some(&i) = f.const_map.get(&v.to_bits()) {
            return i;
        }
        let i = f.constants.len() as u32;
        f.constants.push(v);
        f.const_map.insert(v.to_bits(), i);
        i
    }

    fn str_const(&mut self, s: &str) -> u32 {
        let r = self.heap.intern(s);
        self.constant(Value::string(r))
    }

    fn load_number(&mut self, dst: Reg, n: f64) {
        let idx = self.constant(Value::number(n));
        self.emit(Op::LoadConst { dst, idx });
    }

    fn load_str(&mut self, dst: Reg, s: &str) {
        let idx = self.str_const(s);
        self.emit(Op::LoadConst { dst, idx });
    }

    // ------------------------------------------------------------ escopos

    fn push_scope(&mut self) {
        let f = self.f();
        let scope = Scope {
            names: Vec::new(),
            saved_next: f.next_reg,
            saved_locals_top: f.locals_top,
        };
        f.scopes.push(scope);
    }

    fn pop_scope(&mut self) {
        let f = self.f();
        let s = f.scopes.pop().expect("escopo");
        f.next_reg = s.saved_next;
        f.locals_top = s.saved_locals_top;
    }

    fn at_script_top(&self) -> bool {
        let f = self.fns.last().expect("função atual");
        f.is_script && f.scopes.len() == 1
    }

    fn declare_local(&mut self, name: &str) -> CResult<Reg> {
        if let Some(&(_, r)) = self
            .fns
            .last()
            .and_then(|f| f.scopes.last())
            .and_then(|s| s.names.iter().find(|(n, _)| n == name))
        {
            return Ok(r);
        }
        let r = self.alloc()?;
        let f = self.f();
        f.locals_top = f.locals_top.max(f.next_reg);
        f.scopes
            .last_mut()
            .expect("escopo")
            .names
            .push((name.to_owned(), r));
        Ok(r)
    }

    fn resolve(&self, name: &str) -> Res {
        let (current, outer) = self.fns.split_last().expect("função atual");
        for scope in current.scopes.iter().rev() {
            if let Some(&(_, r)) = scope.names.iter().rev().find(|(n, _)| n == name) {
                return Res::Local(r);
            }
        }
        let captured = outer.iter().any(|f| {
            f.scopes
                .iter()
                .any(|s| s.names.iter().any(|(n, _)| n == name))
        });
        if captured { Res::Captured } else { Res::Global }
    }

    fn captured_error<T>(name: &str) -> CResult<T> {
        unsupported(format!(
            "closure: função interna usando a variável local '{name}' de outra função"
        ))
    }

    fn load_name(&mut self, name: &str, dst: Reg) -> CResult<()> {
        match self.resolve(name) {
            Res::Local(r) => {
                if r != dst {
                    self.emit(Op::Move { dst, src: r });
                }
            }
            Res::Global if name == "undefined" => {
                self.emit(Op::LoadUndefined { dst });
            }
            Res::Global => {
                let idx = self.str_const(name);
                self.emit(Op::GetGlobal { dst, name: idx });
            }
            Res::Captured => return Self::captured_error(name),
        }
        Ok(())
    }

    fn store_name(&mut self, name: &str, src: Reg) -> CResult<()> {
        match self.resolve(name) {
            Res::Local(r) => {
                if r != src {
                    self.emit(Op::Move { dst: r, src });
                }
            }
            Res::Global => {
                let idx = self.str_const(name);
                self.emit(Op::SetGlobal { name: idx, src });
            }
            Res::Captured => return Self::captured_error(name),
        }
        Ok(())
    }

    // ------------------------------------------------------------ programa

    fn program(&mut self, p: &Program<'_>) -> CResult<()> {
        let completion = self.alloc()?;
        self.f().locals_top = 1;
        self.emit(Op::LoadUndefined { dst: completion });
        let mut vars = Vec::new();
        p.body.iter().for_each(|s| collect_vars(s, &mut vars));
        for v in vars {
            let name = self.str_const(v);
            self.emit(Op::DeclareGlobal { name });
        }
        self.hoist_functions(&p.body)?;
        for s in &p.body {
            self.statement(s)?;
        }
        self.emit(Op::Return { src: completion });
        Ok(())
    }

    /// Cria as funções declaradas numa lista de comandos antes de rodar o resto
    /// (é o "hoisting" de `function f() {}`).
    fn hoist_functions(&mut self, stmts: &[Statement<'_>]) -> CResult<()> {
        for s in stmts {
            let Statement::FunctionDeclaration(func) = s else {
                continue;
            };
            let Some(id) = &func.id else { continue };
            let name = id.name.as_str();
            if self.at_script_top() {
                let idx = self.function_decl(func, name)?;
                let m = self.mark();
                let t = self.alloc()?;
                self.emit(Op::Closure { dst: t, func: idx });
                let n = self.str_const(name);
                self.emit(Op::SetGlobal { name: n, src: t });
                self.release(m);
            } else {
                let r = self.declare_local(name)?;
                let idx = self.function_decl(func, name)?;
                self.emit(Op::Closure { dst: r, func: idx });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- funções

    fn function_decl(&mut self, func: &Function<'_>, name: &str) -> CResult<u32> {
        if func.generator {
            return unsupported("generators (function*)");
        }
        if func.r#async {
            return unsupported("funções async");
        }
        self.function(name, &func.params, func.body.as_deref(), None, false)
    }

    fn function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
        expr_body: Option<&Expression<'_>>,
        is_arrow: bool,
    ) -> CResult<u32> {
        if params.rest.is_some() {
            return unsupported("parâmetro rest (...args)");
        }
        let mut names = Vec::with_capacity(params.items.len());
        for p in &params.items {
            if p.initializer.is_some() {
                return unsupported("parâmetro com valor padrão");
            }
            let Some(n) = binding_name(&p.pattern) else {
                return unsupported("desestruturação em parâmetros");
            };
            names.push(n);
        }
        let param_count = u16::try_from(names.len())
            .map_err(|_| JsError::Unsupported("parâmetros demais".into()))?;
        self.fns
            .push(FnState::new(name, param_count, is_arrow, false));
        // parâmetros: registradores 0..n, na ordem
        for n in names {
            let r = self.alloc()?;
            let f = self.f();
            f.locals_top = f.next_reg;
            f.scopes[0].names.push((n.to_owned(), r));
        }
        if let Some(body) = body {
            let mut vars = Vec::new();
            body.statements
                .iter()
                .for_each(|s| collect_vars(s, &mut vars));
            for v in vars {
                self.declare_local(v)?;
            }
            self.hoist_functions(&body.statements)?;
            for s in &body.statements {
                self.statement(s)?;
            }
            let t = self.alloc()?;
            self.emit(Op::LoadUndefined { dst: t });
            self.emit(Op::Return { src: t });
        } else if let Some(e) = expr_body {
            let r = self.alloc()?;
            self.expr(e, r)?;
            self.emit(Op::Return { src: r });
        }
        let state = self.fns.pop().expect("estado da função");
        let code = Rc::new(state.finish());
        let f = self.f();
        f.functions.push(code);
        Ok((f.functions.len() - 1) as u32)
    }

    // ------------------------------------------------------------ comandos

    fn statement(&mut self, s: &Statement<'_>) -> CResult<()> {
        let m = self.mark();
        self.statement_inner(s)?;
        self.release(m);
        Ok(())
    }

    fn statement_inner(&mut self, s: &Statement<'_>) -> CResult<()> {
        match s {
            Statement::ExpressionStatement(e) => {
                if self.fns.last().is_some_and(|f| f.is_script) {
                    self.expr(&e.expression, 0)?;
                } else {
                    self.expr_any(&e.expression)?;
                }
            }
            Statement::VariableDeclaration(d) => self.var_decl(d)?,
            // já criada no hoisting
            Statement::FunctionDeclaration(_) => {}
            Statement::EmptyStatement(_) | Statement::DebuggerStatement(_) => {}
            Statement::BlockStatement(b) => self.block(&b.body)?,
            Statement::ReturnStatement(r) => {
                let src = match &r.argument {
                    Some(e) => self.expr_any(e)?,
                    None => {
                        let t = self.alloc()?;
                        self.emit(Op::LoadUndefined { dst: t });
                        t
                    }
                };
                self.emit(Op::Return { src });
            }
            Statement::IfStatement(i) => {
                let cond = self.expr_any(&i.test)?;
                let j_else = self.emit(Op::JumpIfFalse { cond, target: 0 });
                self.statement(&i.consequent)?;
                if let Some(alt) = &i.alternate {
                    let j_end = self.emit(Op::Jump { target: 0 });
                    self.patch_here(j_else);
                    self.statement(alt)?;
                    self.patch_here(j_end);
                } else {
                    self.patch_here(j_else);
                }
            }
            Statement::WhileStatement(w) => {
                let start = self.here();
                let m = self.mark();
                let cond = self.expr_any(&w.test)?;
                let j_end = self.emit(Op::JumpIfFalse { cond, target: 0 });
                self.release(m);
                self.push_loop(true);
                self.statement(&w.body)?;
                self.emit(Op::Jump { target: start });
                let end = self.here();
                self.patch(j_end, end);
                self.pop_loop(start, end);
            }
            Statement::DoWhileStatement(w) => {
                let start = self.here();
                self.push_loop(true);
                self.statement(&w.body)?;
                let cont = self.here();
                let cond = self.expr_any(&w.test)?;
                self.emit(Op::JumpIfTrue {
                    cond,
                    target: start,
                });
                let end = self.here();
                self.pop_loop(cont, end);
            }
            Statement::ForStatement(fs) => {
                self.push_scope();
                match &fs.init {
                    Some(ForStatementInit::VariableDeclaration(d)) => self.var_decl(d)?,
                    Some(init) => {
                        if let Some(e) = init.as_expression() {
                            let m = self.mark();
                            self.expr_any(e)?;
                            self.release(m);
                        }
                    }
                    None => {}
                }
                let start = self.here();
                let mut j_end = None;
                if let Some(test) = &fs.test {
                    let m = self.mark();
                    let cond = self.expr_any(test)?;
                    j_end = Some(self.emit(Op::JumpIfFalse { cond, target: 0 }));
                    self.release(m);
                }
                self.push_loop(true);
                self.statement(&fs.body)?;
                let cont = self.here();
                if let Some(update) = &fs.update {
                    let m = self.mark();
                    self.expr_any(update)?;
                    self.release(m);
                }
                self.emit(Op::Jump { target: start });
                let end = self.here();
                if let Some(j) = j_end {
                    self.patch(j, end);
                }
                self.pop_loop(cont, end);
                self.pop_scope();
            }
            Statement::BreakStatement(b) => {
                if b.label.is_some() {
                    return unsupported("break com rótulo");
                }
                self.jump_out(false)?;
            }
            Statement::ContinueStatement(c) => {
                if c.label.is_some() {
                    return unsupported("continue com rótulo");
                }
                self.jump_out(true)?;
            }
            Statement::ThrowStatement(t) => {
                let src = self.expr_any(&t.argument)?;
                self.emit(Op::Throw { src });
            }
            Statement::TryStatement(t) => self.try_statement(t)?,
            Statement::SwitchStatement(sw) => self.switch(sw)?,
            Statement::LabeledStatement(l) => self.statement(&l.body)?,
            Statement::ClassDeclaration(_) => return unsupported("classes"),
            Statement::ForInStatement(_) => return unsupported("for-in"),
            Statement::ForOfStatement(_) => return unsupported("for-of"),
            Statement::WithStatement(_) => return unsupported("with"),
            _ => return unsupported("declaração de módulo ou TypeScript"),
        }
        Ok(())
    }

    fn block(&mut self, body: &[Statement<'_>]) -> CResult<()> {
        self.push_scope();
        self.hoist_functions(body)?;
        for s in body {
            self.statement(s)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn var_decl(&mut self, d: &VariableDeclaration<'_>) -> CResult<()> {
        let lexical = d.kind != VariableDeclarationKind::Var;
        for decl in &d.declarations {
            let Some(name) = binding_name(&decl.id) else {
                return unsupported("desestruturação em declarações");
            };
            if lexical && !self.at_script_top() {
                let r = self.declare_local(name)?;
                match &decl.init {
                    Some(e) => self.expr(e, r)?,
                    None => {
                        self.emit(Op::LoadUndefined { dst: r });
                    }
                }
                continue;
            }
            if lexical {
                let n = self.str_const(name);
                self.emit(Op::DeclareGlobal { name: n });
            }
            let m = self.mark();
            match &decl.init {
                Some(e) => {
                    let t = match self.resolve(name) {
                        Res::Local(r) => r,
                        _ => self.alloc()?,
                    };
                    self.expr(e, t)?;
                    self.store_name(name, t)?;
                }
                None if lexical => {
                    let t = self.alloc()?;
                    self.emit(Op::LoadUndefined { dst: t });
                    self.store_name(name, t)?;
                }
                None => {}
            }
            self.release(m);
        }
        Ok(())
    }

    fn push_loop(&mut self, accepts_continue: bool) {
        let f = self.f();
        let try_depth = f.try_depth;
        f.loops.push(LoopCtx {
            breaks: Vec::new(),
            continues: Vec::new(),
            try_depth,
            accepts_continue,
        });
    }

    fn pop_loop(&mut self, continue_target: u32, break_target: u32) {
        let ctx = self.f().loops.pop().expect("laço");
        for j in ctx.continues {
            self.patch(j, continue_target);
        }
        for j in ctx.breaks {
            self.patch(j, break_target);
        }
    }

    fn jump_out(&mut self, is_continue: bool) -> CResult<()> {
        let f = self.f();
        let Some(idx) = f
            .loops
            .iter()
            .rposition(|l| !is_continue || l.accepts_continue)
        else {
            return Err(JsError::Syntax("break/continue fora de laço".into()));
        };
        // sair de blocos try no caminho desativa os seus `catch`
        let pops = f.try_depth - f.loops[idx].try_depth;
        for _ in 0..pops {
            self.emit(Op::ExitTry);
        }
        let j = self.emit(Op::Jump { target: 0 });
        let ctx = &mut self.f().loops[idx];
        if is_continue {
            ctx.continues.push(j);
        } else {
            ctx.breaks.push(j);
        }
        Ok(())
    }

    fn switch(&mut self, sw: &SwitchStatement<'_>) -> CResult<()> {
        let disc = self.alloc()?;
        self.expr(&sw.discriminant, disc)?;
        let test = self.alloc()?;
        let mut case_jumps = Vec::new();
        for (i, case) in sw.cases.iter().enumerate() {
            if let Some(t) = &case.test {
                self.expr(t, test)?;
                self.emit(Op::Binary {
                    op: BinOp::StrictEq,
                    dst: test,
                    lhs: disc,
                    rhs: test,
                });
                case_jumps.push((
                    i,
                    self.emit(Op::JumpIfTrue {
                        cond: test,
                        target: 0,
                    }),
                ));
            }
        }
        let j_default = self.emit(Op::Jump { target: 0 });
        let mut default_target = None;
        self.push_loop(false);
        self.push_scope();
        for (i, case) in sw.cases.iter().enumerate() {
            let label = self.here();
            if case.test.is_none() {
                default_target = Some(label);
            }
            for &(ci, j) in &case_jumps {
                if ci == i {
                    self.patch(j, label);
                }
            }
            self.hoist_functions(&case.consequent)?;
            for s in &case.consequent {
                self.statement(s)?;
            }
        }
        self.pop_scope();
        let end = self.here();
        self.patch(j_default, default_target.unwrap_or(end));
        self.pop_loop(end, end);
        Ok(())
    }

    fn try_statement(&mut self, t: &TryStatement<'_>) -> CResult<()> {
        let exc = self.alloc()?;
        let enter = self.emit(Op::EnterTry {
            catch_target: 0,
            exc,
        });
        self.f().try_depth += 1;
        self.block(&t.block.body)?;
        self.f().try_depth -= 1;
        self.emit(Op::ExitTry);
        if let Some(fin) = &t.finalizer {
            self.block(&fin.body)?;
        }
        let j_end = self.emit(Op::Jump { target: 0 });
        self.patch_here(enter);

        match &t.handler {
            Some(handler) => {
                // exceção dentro do catch ainda precisa rodar o finally
                let mut inner = None;
                if t.finalizer.is_some() {
                    let exc2 = self.alloc()?;
                    let e2 = self.emit(Op::EnterTry {
                        catch_target: 0,
                        exc: exc2,
                    });
                    inner = Some((e2, exc2));
                    self.f().try_depth += 1;
                }
                self.push_scope();
                if let Some(param) = &handler.param {
                    let Some(name) = binding_name(&param.pattern) else {
                        return unsupported("desestruturação no catch");
                    };
                    let f = self.f();
                    f.scopes
                        .last_mut()
                        .expect("escopo")
                        .names
                        .push((name.to_owned(), exc));
                }
                self.hoist_functions(&handler.body.body)?;
                for s in &handler.body.body {
                    self.statement(s)?;
                }
                self.pop_scope();
                if let Some((e2, exc2)) = inner {
                    self.f().try_depth -= 1;
                    self.emit(Op::ExitTry);
                    let fin = t.finalizer.as_ref().expect("finally");
                    self.block(&fin.body)?;
                    let j = self.emit(Op::Jump { target: 0 });
                    self.patch_here(e2);
                    self.block(&fin.body)?;
                    self.emit(Op::Throw { src: exc2 });
                    self.patch_here(j);
                }
            }
            None => {
                // try/finally sem catch: roda o finally e relança
                if let Some(fin) = &t.finalizer {
                    self.block(&fin.body)?;
                }
                self.emit(Op::Throw { src: exc });
            }
        }
        self.patch_here(j_end);
        Ok(())
    }

    // ---------------------------------------------------------- expressões

    /// Avalia `e` e devolve um registrador com o resultado (sem cópia se `e` for
    /// uma variável local).
    fn expr_any(&mut self, e: &Expression<'_>) -> CResult<Reg> {
        if let Expression::Identifier(id) = e
            && let Res::Local(r) = self.resolve(id.name.as_str())
        {
            return Ok(r);
        }
        let t = self.alloc()?;
        self.expr(e, t)?;
        Ok(t)
    }

    /// Avalia `e` deixando o resultado em `dst`.
    fn expr(&mut self, e: &Expression<'_>, dst: Reg) -> CResult<()> {
        let m = self.mark();
        self.expr_inner(e, dst)?;
        self.release(m);
        Ok(())
    }

    fn expr_inner(&mut self, e: &Expression<'_>, dst: Reg) -> CResult<()> {
        match e {
            Expression::NumericLiteral(n) => self.load_number(dst, n.value),
            Expression::StringLiteral(s) => {
                let s = js_string(s.value);
                self.load_str(dst, &s);
            }
            Expression::BooleanLiteral(b) => {
                self.emit(Op::LoadBool {
                    dst,
                    value: b.value,
                });
            }
            Expression::NullLiteral(_) => {
                self.emit(Op::LoadNull { dst });
            }
            Expression::Identifier(id) => self.load_name(id.name.as_str(), dst)?,
            Expression::ThisExpression(_) => {
                self.emit(Op::LoadThis { dst });
            }
            Expression::ParenthesizedExpression(p) => self.expr_inner(&p.expression, dst)?,
            Expression::TemplateLiteral(t) => self.template(t, dst)?,
            Expression::ArrayExpression(a) => {
                self.emit(Op::NewArray { dst });
                for el in &a.elements {
                    let m = self.mark();
                    let r = match el {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return unsupported("spread em array (...x)");
                        }
                        ArrayExpressionElement::Elision(_) => {
                            let t = self.alloc()?;
                            self.emit(Op::LoadUndefined { dst: t });
                            t
                        }
                        other => {
                            let e = other.as_expression().expect("elemento é expressão");
                            self.expr_any(e)?
                        }
                    };
                    self.emit(Op::ArrayPush { array: dst, src: r });
                    self.release(m);
                }
            }
            Expression::ObjectExpression(o) => self.object(o, dst)?,
            Expression::FunctionExpression(func) => {
                let name = func.id.as_ref().map(|i| i.name.as_str()).unwrap_or("");
                let idx = self.function_decl(func, name)?;
                self.emit(Op::Closure { dst, func: idx });
            }
            Expression::ArrowFunctionExpression(a) => {
                if a.r#async {
                    return unsupported("funções async");
                }
                let idx = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => {
                        self.function("", &a.params, Some(&**b), None, true)?
                    }
                    other => {
                        let e = other.as_expression().expect("corpo de arrow é expressão");
                        self.function("", &a.params, None, Some(e), true)?
                    }
                };
                self.emit(Op::Closure { dst, func: idx });
            }
            Expression::UnaryExpression(u) => self.unary(u, dst)?,
            Expression::UpdateExpression(u) => self.update(u, dst)?,
            Expression::BinaryExpression(b) => {
                let lhs = if is_simple(&b.right) {
                    self.expr_any(&b.left)?
                } else {
                    // o lado direito pode mudar a variável da esquerda: copia antes
                    let t = self.alloc()?;
                    self.expr(&b.left, t)?;
                    t
                };
                let rhs = self.expr_any(&b.right)?;
                self.emit(Op::Binary {
                    op: map_binary(b.operator),
                    dst,
                    lhs,
                    rhs,
                });
            }
            Expression::LogicalExpression(l) => {
                self.expr(&l.left, dst)?;
                let j = self.logical_jump(l.operator, dst);
                self.expr(&l.right, dst)?;
                self.patch_here(j);
            }
            Expression::ConditionalExpression(c) => {
                let cond = self.expr_any(&c.test)?;
                let j_else = self.emit(Op::JumpIfFalse { cond, target: 0 });
                self.expr(&c.consequent, dst)?;
                let j_end = self.emit(Op::Jump { target: 0 });
                self.patch_here(j_else);
                self.expr(&c.alternate, dst)?;
                self.patch_here(j_end);
            }
            Expression::SequenceExpression(s) => {
                for e in &s.expressions {
                    self.expr(e, dst)?;
                }
            }
            Expression::AssignmentExpression(a) => self.assign(a, dst)?,
            Expression::CallExpression(c) => self.call(c, dst)?,
            Expression::NewExpression(n) => {
                let callee = self.alloc()?;
                self.expr(&n.callee, callee)?;
                let (args, argc) = self.arguments(&n.arguments)?;
                self.emit(Op::New {
                    dst,
                    callee,
                    args,
                    argc,
                });
            }
            Expression::StaticMemberExpression(me) => {
                if me.optional {
                    return unsupported("encadeamento opcional (?.)");
                }
                let obj = self.expr_any(&me.object)?;
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::GetNamed { dst, obj, name });
            }
            Expression::ComputedMemberExpression(me) => {
                if me.optional {
                    return unsupported("encadeamento opcional (?.)");
                }
                let obj = if is_simple(&me.expression) {
                    self.expr_any(&me.object)?
                } else {
                    let t = self.alloc()?;
                    self.expr(&me.object, t)?;
                    t
                };
                let key = self.expr_any(&me.expression)?;
                self.emit(Op::GetKeyed { dst, obj, key });
            }
            Expression::PrivateFieldExpression(_) => return unsupported("campos privados (#x)"),
            Expression::ClassExpression(_) => return unsupported("classes"),
            Expression::ChainExpression(_) => return unsupported("encadeamento opcional (?.)"),
            Expression::RegExpLiteral(_) => return unsupported("expressões regulares"),
            Expression::BigIntLiteral(_) => return unsupported("BigInt"),
            Expression::TaggedTemplateExpression(_) => return unsupported("template com tag"),
            Expression::YieldExpression(_) => return unsupported("yield"),
            Expression::AwaitExpression(_) => return unsupported("await"),
            Expression::Super(_) => return unsupported("super"),
            Expression::NewTarget(_) => return unsupported("new.target"),
            Expression::ImportExpression(_) | Expression::ImportMeta(_) => {
                return unsupported("import()");
            }
            _ => return unsupported("expressão (JSX/TypeScript/privada)"),
        }
        Ok(())
    }

    fn logical_jump(&mut self, op: LogicalOperator, cond: Reg) -> usize {
        match op {
            LogicalOperator::And => self.emit(Op::JumpIfFalse { cond, target: 0 }),
            LogicalOperator::Or => self.emit(Op::JumpIfTrue { cond, target: 0 }),
            LogicalOperator::Coalesce => self.emit(Op::JumpIfNotNullish { cond, target: 0 }),
        }
    }

    fn template(&mut self, t: &TemplateLiteral<'_>, dst: Reg) -> CResult<()> {
        let cooked = |q: &TemplateElement<'_>| match q.value.cooked {
            Some(c) => js_string(c),
            None => q.value.raw.as_str().to_owned(),
        };
        let first = t.quasis.first().map(cooked).unwrap_or_default();
        self.load_str(dst, &first);
        for (i, e) in t.expressions.iter().enumerate() {
            let m = self.mark();
            let r = self.expr_any(e)?;
            self.emit(Op::Binary {
                op: BinOp::Add,
                dst,
                lhs: dst,
                rhs: r,
            });
            if let Some(q) = t.quasis.get(i + 1) {
                let s = cooked(q);
                if !s.is_empty() {
                    let tmp = self.alloc()?;
                    self.load_str(tmp, &s);
                    self.emit(Op::Binary {
                        op: BinOp::Add,
                        dst,
                        lhs: dst,
                        rhs: tmp,
                    });
                }
            }
            self.release(m);
        }
        Ok(())
    }

    fn object(&mut self, o: &ObjectExpression<'_>, dst: Reg) -> CResult<()> {
        self.emit(Op::NewObject { dst });
        for prop in &o.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                return unsupported("spread em objeto ({...x})");
            };
            if p.kind != PropertyKind::Init {
                return unsupported("getter/setter em objeto literal");
            }
            let m = self.mark();
            let static_key = if p.computed {
                None
            } else {
                match &p.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.as_str().to_owned()),
                    PropertyKey::StringLiteral(s) => Some(js_string(s.value)),
                    PropertyKey::NumericLiteral(n) => Some(number_to_string(n.value)),
                    PropertyKey::PrivateIdentifier(_) => {
                        return unsupported("campos privados (#x)");
                    }
                    _ => None,
                }
            };
            match static_key {
                Some(key) => {
                    let v = self.expr_any(&p.value)?;
                    let name = self.str_const(&key);
                    self.emit(Op::SetNamed {
                        obj: dst,
                        name,
                        src: v,
                    });
                }
                None => {
                    let Some(ke) = p.key.as_expression() else {
                        return unsupported("chave de propriedade");
                    };
                    let k = self.alloc()?;
                    self.expr(ke, k)?;
                    let v = self.expr_any(&p.value)?;
                    self.emit(Op::SetKeyed {
                        obj: dst,
                        key: k,
                        src: v,
                    });
                }
            }
            self.release(m);
        }
        Ok(())
    }

    fn unary(&mut self, u: &UnaryExpression<'_>, dst: Reg) -> CResult<()> {
        let op = match u.operator {
            UnaryOperator::UnaryNegation => UnOp::Neg,
            UnaryOperator::UnaryPlus => UnOp::Plus,
            UnaryOperator::LogicalNot => UnOp::Not,
            UnaryOperator::BitwiseNot => UnOp::BitNot,
            UnaryOperator::Void => UnOp::Void,
            UnaryOperator::Typeof => {
                // `typeof x` com x global inexistente não lança erro
                if let Expression::Identifier(id) = &u.argument
                    && let Res::Global = self.resolve(id.name.as_str())
                {
                    let name = self.str_const(id.name.as_str());
                    self.emit(Op::TypeOfGlobal { dst, name });
                    return Ok(());
                }
                UnOp::TypeOf
            }
            UnaryOperator::Delete => return unsupported("delete"),
        };
        let src = self.expr_any(&u.argument)?;
        self.emit(Op::Unary { op, dst, src });
        Ok(())
    }

    fn update(&mut self, u: &UpdateExpression<'_>, dst: Reg) -> CResult<()> {
        let op = match u.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let old = self.alloc()?;
        let new = self.alloc()?;
        // lê o valor atual, converte para número, soma/subtrai 1 e grava
        enum Target {
            Name(String),
            Named(Reg, u32),
            Keyed(Reg, Reg),
        }
        let target = match &u.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                self.load_name(name, old)?;
                Target::Name(name.to_owned())
            }
            SimpleAssignmentTarget::StaticMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::GetNamed {
                    dst: old,
                    obj,
                    name,
                });
                Target::Named(obj, name)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let key = self.alloc()?;
                self.expr(&me.expression, key)?;
                self.emit(Op::GetKeyed { dst: old, obj, key });
                Target::Keyed(obj, key)
            }
            _ => return unsupported("++/-- neste tipo de alvo"),
        };
        self.emit(Op::Unary {
            op: UnOp::Plus,
            dst: old,
            src: old,
        });
        self.load_number(new, 1.0);
        self.emit(Op::Binary {
            op,
            dst: new,
            lhs: old,
            rhs: new,
        });
        match target {
            Target::Name(name) => self.store_name(&name, new)?,
            Target::Named(obj, name) => {
                self.emit(Op::SetNamed {
                    obj,
                    name,
                    src: new,
                });
            }
            Target::Keyed(obj, key) => {
                self.emit(Op::SetKeyed { obj, key, src: new });
            }
        }
        let src = if u.prefix { new } else { old };
        self.emit(Op::Move { dst, src });
        Ok(())
    }

    fn assign(&mut self, a: &AssignmentExpression<'_>, dst: Reg) -> CResult<()> {
        let op = a.operator;
        let plain = op == AssignmentOperator::Assign;
        match &a.left {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if plain {
                    match self.resolve(name) {
                        Res::Local(r) => {
                            self.expr(&a.right, r)?;
                            if r != dst {
                                self.emit(Op::Move { dst, src: r });
                            }
                        }
                        Res::Global => {
                            self.expr(&a.right, dst)?;
                            self.store_name(name, dst)?;
                        }
                        Res::Captured => return Self::captured_error(name),
                    }
                    return Ok(());
                }
                self.load_name(name, dst)?;
                self.compound(op, dst, &a.right)?;
                self.store_name(name, dst)?;
            }
            AssignmentTarget::StaticMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let name = self.str_const(me.property.name.as_str());
                if plain {
                    self.expr(&a.right, dst)?;
                } else {
                    self.emit(Op::GetNamed { dst, obj, name });
                    self.compound(op, dst, &a.right)?;
                }
                self.emit(Op::SetNamed {
                    obj,
                    name,
                    src: dst,
                });
            }
            AssignmentTarget::ComputedMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let key = self.alloc()?;
                self.expr(&me.expression, key)?;
                if plain {
                    self.expr(&a.right, dst)?;
                } else {
                    self.emit(Op::GetKeyed { dst, obj, key });
                    self.compound(op, dst, &a.right)?;
                }
                self.emit(Op::SetKeyed { obj, key, src: dst });
            }
            _ => return unsupported("atribuição com desestruturação"),
        }
        Ok(())
    }

    /// `x op= y` com o valor atual de `x` já em `dst`; deixa o novo valor em `dst`.
    /// Para `&&=`, `||=` e `??=` o lado direito só roda se precisar.
    fn compound(
        &mut self,
        op: AssignmentOperator,
        dst: Reg,
        right: &Expression<'_>,
    ) -> CResult<()> {
        if let Some(lop) = op.to_logical_operator() {
            let j = self.logical_jump(lop, dst);
            self.expr(right, dst)?;
            self.patch_here(j);
            return Ok(());
        }
        let Some(bop) = op.to_binary_operator() else {
            return unsupported("operador de atribuição");
        };
        let m = self.mark();
        let rhs = self.expr_any(right)?;
        self.emit(Op::Binary {
            op: map_binary(bop),
            dst,
            lhs: dst,
            rhs,
        });
        self.release(m);
        Ok(())
    }

    fn call(&mut self, c: &CallExpression<'_>, dst: Reg) -> CResult<()> {
        if c.optional {
            return unsupported("chamada opcional (?.())");
        }
        let callee = self.alloc()?;
        let this = self.alloc()?;
        match &c.callee {
            Expression::StaticMemberExpression(me) if !me.optional => {
                self.expr(&me.object, this)?;
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::GetNamed {
                    dst: callee,
                    obj: this,
                    name,
                });
            }
            Expression::ComputedMemberExpression(me) if !me.optional => {
                self.expr(&me.object, this)?;
                let m = self.mark();
                let key = self.expr_any(&me.expression)?;
                self.emit(Op::GetKeyed {
                    dst: callee,
                    obj: this,
                    key,
                });
                self.release(m);
            }
            other => {
                self.expr(other, callee)?;
                self.emit(Op::LoadUndefined { dst: this });
            }
        }
        let (args, argc) = self.arguments(&c.arguments)?;
        self.emit(Op::Call {
            dst,
            callee,
            this,
            args,
            argc,
        });
        Ok(())
    }

    /// Avalia os argumentos em registradores consecutivos; devolve (primeiro, quantos).
    fn arguments(&mut self, list: &[Argument<'_>]) -> CResult<(Reg, u16)> {
        let start = self.mark();
        let mut regs = Vec::with_capacity(list.len());
        for _ in list {
            regs.push(self.alloc()?);
        }
        for (a, r) in list.iter().zip(regs) {
            let Some(e) = a.as_expression() else {
                return unsupported("spread em argumentos (...x)");
            };
            self.expr(e, r)?;
        }
        let argc = u16::try_from(list.len())
            .map_err(|_| JsError::Unsupported("argumentos demais".into()))?;
        Ok((start, argc))
    }
}
