//! Compilador: AST do `oxc` → bytecode de registradores.
//!
//! Cada função tem sua janela de registradores: parâmetros posicionais, eventual
//! parâmetro `...rest`, variáveis locais (`var` sobe para a função; `let`/`const`/`class`
//! vivem no bloco) e temporários das expressões. Variáveis locais capturadas por
//! funções internas usam upvalues (`GetUpvalue` / `SetUpvalue` / `CloseUpvalues`).

use std::collections::HashMap;
use std::rc::Rc;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_str::JSStr;

use crate::bytecode::{BinOp, FunctionCode, Op, Reg, UnOp, UpvalueDesc};
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
        fns: vec![FnState::new("<script>", 0, 0, false, false, false, false, true)],
        pending_fields: Vec::new(),
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
    continue_locals_from: u16,
    break_locals_from: u16,
    /// `switch` aceita `break` mas não `continue`.
    accepts_continue: bool,
}

struct FnState {
    name: String,
    params: u16,
    length: u16,
    has_rest: bool,
    is_arrow: bool,
    is_class_ctor: bool,
    is_derived_ctor: bool,
    is_script: bool,
    code: Vec<Op>,
    constants: Vec<Value>,
    const_map: HashMap<u64, u32>,
    functions: Vec<Rc<FunctionCode>>,
    upvalues: Vec<UpvalueDesc>,
    scopes: Vec<Scope>,
    next_reg: u16,
    max_reg: u16,
    /// Nenhum temporário pode ser liberado abaixo disto (há variáveis locais vivas).
    locals_top: u16,
    loops: Vec<LoopCtx>,
    try_depth: usize,
}

impl FnState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: &str,
        params: u16,
        length: u16,
        has_rest: bool,
        is_arrow: bool,
        is_class_ctor: bool,
        is_derived_ctor: bool,
        is_script: bool,
    ) -> Self {
        FnState {
            name: name.to_owned(),
            params,
            length,
            has_rest,
            is_arrow,
            is_class_ctor,
            is_derived_ctor,
            is_script,
            code: Vec::new(),
            constants: Vec::new(),
            const_map: HashMap::new(),
            functions: Vec::new(),
            upvalues: Vec::new(),
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
            length: self.length,
            has_rest: self.has_rest,
            registers: self.max_reg.max(1),
            code: self.code,
            constants: self.constants,
            functions: self.functions,
            upvalues: self.upvalues,
            is_arrow: self.is_arrow,
            is_class_ctor: self.is_class_ctor,
            is_derived_ctor: self.is_derived_ctor,
        }
    }
}

enum Res {
    Local(Reg),
    Upvalue(u16),
    Global,
}

struct Compiler<'h, 'a> {
    heap: &'h mut Heap,
    fns: Vec<FnState>,
    /// Campos de instância pendentes para injetar logo após `super()` (ou no início do ctor).
    pending_fields: Vec<(usize, Vec<&'a PropertyDefinition<'a>>)>,
}

fn js_string(s: JSStr<'_>) -> String {
    match s.as_str() {
        Some(s) => s.to_owned(),
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

// ------------------------------------------------- hoisting de `var` e nomes léxicos

fn collect_pattern_names<'a>(p: &BindingPattern<'a>, out: &mut Vec<&'a str>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.as_str()),
        BindingPattern::AssignmentPattern(ap) => collect_pattern_names(&ap.left, out),
        BindingPattern::ArrayPattern(ap) => {
            for el in ap.elements.iter().flatten() {
                collect_pattern_names(el, out);
            }
            if let Some(r) = &ap.rest {
                collect_pattern_names(&r.argument, out);
            }
        }
        BindingPattern::ObjectPattern(op) => {
            for prop in &op.properties {
                collect_pattern_names(&prop.value, out);
            }
            if let Some(r) = &op.rest {
                collect_pattern_names(&r.argument, out);
            }
        }
    }
}

fn var_names<'a>(d: &VariableDeclaration<'a>, out: &mut Vec<&'a str>) {
    if d.kind == VariableDeclarationKind::Var {
        for decl in &d.declarations {
            collect_pattern_names(&decl.id, out);
        }
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

/// Junta nomes de `let`/`const`/`class` diretamente no nível do bloco atual (não recursivo).
fn collect_lexical<'a>(stmts: &[Statement<'a>], out: &mut Vec<&'a str>) {
    for s in stmts {
        match s {
            Statement::VariableDeclaration(d) if d.kind != VariableDeclarationKind::Var => {
                for decl in &d.declarations {
                    collect_pattern_names(&decl.id, out);
                }
            }
            Statement::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    out.push(id.name.as_str());
                }
            }
            _ => {}
        }
    }
}

fn add_upvalue(fn_state: &mut FnState, is_local: bool, index: u16) -> CResult<u16> {
    if let Some(pos) = fn_state
        .upvalues
        .iter()
        .position(|u| u.is_local == is_local && u.index == index)
    {
        return Ok(pos as u16);
    }
    let idx = u16::try_from(fn_state.upvalues.len())
        .map_err(|_| JsError::Unsupported("upvalues demais".into()))?;
    fn_state.upvalues.push(UpvalueDesc { is_local, index });
    Ok(idx)
}

impl<'h, 'a> Compiler<'h, 'a> {
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
            | Op::JumpIfNotNullish { target, .. }
            | Op::JumpIfNullish { target, .. }
            | Op::JumpIfNotUndefined { target, .. } => *target = to,
            Op::EnterTry { catch_target, .. } => *catch_target = to,
            Op::IterNext { done_target, .. } => *done_target = to,
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

    // ------------------------------------------------------------ escopos e upvalues

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
        let close_from = {
            let f = self.f();
            let s = f.scopes.pop().expect("escopo");
            let had_locals = f.locals_top > s.saved_locals_top || !s.names.is_empty();
            let from = s.saved_next;
            f.next_reg = s.saved_next;
            f.locals_top = s.saved_locals_top;
            had_locals.then_some(from)
        };
        if let Some(from_reg) = close_from {
            self.emit(Op::CloseUpvalues { from_reg });
        }
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

    fn resolve_upvalue(&mut self, fn_idx: usize, name: &str) -> CResult<Option<u16>> {
        if fn_idx == 0 {
            return Ok(None);
        }
        let parent = fn_idx - 1;
        for scope in self.fns[parent].scopes.iter().rev() {
            if let Some(&(_, reg)) = scope.names.iter().rev().find(|(n, _)| n == name) {
                let uv = add_upvalue(&mut self.fns[fn_idx], true, reg)?;
                return Ok(Some(uv));
            }
        }
        if let Some(parent_uv) = self.resolve_upvalue(parent, name)? {
            let uv = add_upvalue(&mut self.fns[fn_idx], false, parent_uv)?;
            return Ok(Some(uv));
        }
        Ok(None)
    }

    fn resolve(&mut self, name: &str) -> CResult<Res> {
        let last = self.fns.len() - 1;
        for scope in self.fns[last].scopes.iter().rev() {
            if let Some(&(_, r)) = scope.names.iter().rev().find(|(n, _)| n == name) {
                return Ok(Res::Local(r));
            }
        }
        if let Some(uv) = self.resolve_upvalue(last, name)? {
            return Ok(Res::Upvalue(uv));
        }
        Ok(Res::Global)
    }

    fn load_name(&mut self, name: &str, dst: Reg) -> CResult<()> {
        match self.resolve(name)? {
            Res::Local(r) => {
                if r != dst {
                    self.emit(Op::Move { dst, src: r });
                }
            }
            Res::Upvalue(uv) => {
                self.emit(Op::GetUpvalue { dst, upvalue: uv });
            }
            Res::Global if name == "undefined" => {
                self.emit(Op::LoadUndefined { dst });
            }
            Res::Global => {
                let idx = self.str_const(name);
                self.emit(Op::GetGlobal { dst, name: idx });
            }
        }
        Ok(())
    }

    fn store_name(&mut self, name: &str, src: Reg) -> CResult<()> {
        match self.resolve(name)? {
            Res::Local(r) => {
                if r != src {
                    self.emit(Op::Move { dst: r, src });
                }
            }
            Res::Upvalue(uv) => {
                self.emit(Op::SetUpvalue { upvalue: uv, src });
            }
            Res::Global => {
                let idx = self.str_const(name);
                self.emit(Op::SetGlobal { name: idx, src });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ programa

    fn program(&mut self, p: &'a Program<'a>) -> CResult<()> {
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

    /// Cria as funções declaradas numa lista de comandos antes de rodar o resto.
    fn hoist_functions(&mut self, stmts: &'a [Statement<'a>]) -> CResult<()> {
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

    fn pre_declare_lexical(&mut self, stmts: &[Statement<'a>]) -> CResult<()> {
        if self.at_script_top() {
            return Ok(());
        }
        let mut lex = Vec::new();
        collect_lexical(stmts, &mut lex);
        for name in lex {
            self.declare_local(name)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- funções

    fn function_decl(&mut self, func: &'a Function<'a>, name: &str) -> CResult<u32> {
        if func.generator {
            return unsupported("generators (function*)");
        }
        if func.r#async {
            return unsupported("funções async");
        }
        self.function(
            name,
            &func.params,
            func.body.as_deref(),
            None,
            false,
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn function(
        &mut self,
        name: &str,
        params: &'a FormalParameters<'a>,
        body: Option<&'a FunctionBody<'a>>,
        expr_body: Option<&'a Expression<'a>>,
        is_arrow: bool,
        is_class_ctor: bool,
        is_derived_ctor: bool,
    ) -> CResult<u32> {
        let param_count = u16::try_from(params.items.len())
            .map_err(|_| JsError::Unsupported("parâmetros demais".into()))?;
        let has_rest = params.rest.is_some();
        let js_length = params
            .items
            .iter()
            .take_while(|p| p.initializer.is_none())
            .count() as u16;

        self.fns.push(FnState::new(
            name,
            param_count,
            js_length,
            has_rest,
            is_arrow,
            is_class_ctor,
            is_derived_ctor,
            false,
        ));

        // Reserva registradores posicionais 0..param_count e +1 para rest
        let mut pos_regs = Vec::with_capacity(params.items.len());
        for _ in &params.items {
            pos_regs.push(self.alloc()?);
        }
        let rest_reg = if has_rest {
            Some(self.alloc()?)
        } else {
            None
        };
        let f = self.f();
        f.locals_top = f.next_reg;

        // Inicializa defaults e faz destructuring de parâmetros
        for (p, r) in params.items.iter().zip(pos_regs) {
            if let Some(init) = &p.initializer {
                let skip = self.emit(Op::JumpIfNotUndefined { cond: r, target: 0 });
                self.expr(init, r)?;
                self.patch_here(skip);
            }
            match &p.pattern {
                BindingPattern::BindingIdentifier(id) => {
                    self.f().scopes[0]
                        .names
                        .push((id.name.as_str().to_owned(), r));
                }
                pat => {
                    self.bind_pattern(pat, r, false, false)?;
                }
            }
        }
        if let (Some(rest), Some(r_reg)) = (&params.rest, rest_reg) {
            match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => {
                    self.f().scopes[0]
                        .names
                        .push((id.name.as_str().to_owned(), r_reg));
                }
                pat => {
                    self.bind_pattern(pat, r_reg, false, false)?;
                }
            }
        }

        // Em construtor de classe base, campos de instância rodam antes do corpo
        if is_class_ctor && !is_derived_ctor {
            self.emit_pending_fields_for_current_fn()?;
        }

        if let Some(body) = body {
            let mut vars = Vec::new();
            body.statements
                .iter()
                .for_each(|s| collect_vars(s, &mut vars));
            for v in vars {
                self.declare_local(v)?;
            }
            self.pre_declare_lexical(&body.statements)?;
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

    fn emit_pending_fields_for_current_fn(&mut self) -> CResult<()> {
        let depth = self.fns.len() - 1;
        let Some(pos) = self.pending_fields.iter().rposition(|(d, _)| *d == depth) else {
            return Ok(());
        };
        let fields = self.pending_fields[pos].1.clone();
        if fields.is_empty() {
            return Ok(());
        }
        let m = self.mark();
        let this_reg = self.alloc()?;
        self.emit(Op::LoadThis { dst: this_reg });
        for prop in fields {
            let m2 = self.mark();
            let val_reg = self.alloc()?;
            if let Some(init) = &prop.value {
                self.expr(init, val_reg)?;
            } else {
                self.emit(Op::LoadUndefined { dst: val_reg });
            }
            self.set_property_key(this_reg, &prop.key, prop.computed, val_reg)?;
            self.release(m2);
        }
        self.release(m);
        Ok(())
    }

    // ------------------------------------------------------------ comandos

    fn statement(&mut self, s: &'a Statement<'a>) -> CResult<()> {
        let m = self.mark();
        self.statement_inner(s)?;
        self.release(m);
        Ok(())
    }

    fn statement_inner(&mut self, s: &'a Statement<'a>) -> CResult<()> {
        match s {
            Statement::ExpressionStatement(e) => {
                if self.fns.last().is_some_and(|f| f.is_script) {
                    self.expr(&e.expression, 0)?;
                } else {
                    self.expr_any(&e.expression)?;
                }
            }
            Statement::VariableDeclaration(d) => self.var_decl(d)?,
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
                let ltop = self.f().locals_top;
                self.push_loop(true, ltop, ltop);
                self.statement(&w.body)?;
                self.emit(Op::Jump { target: start });
                let end = self.here();
                self.patch(j_end, end);
                self.pop_loop(start, end);
            }
            Statement::DoWhileStatement(w) => {
                let start = self.here();
                let ltop = self.f().locals_top;
                self.push_loop(true, ltop, ltop);
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
                let has_lexical_init = matches!(
                    &fs.init,
                    Some(ForStatementInit::VariableDeclaration(d)) if d.kind != VariableDeclarationKind::Var
                );
                let loop_vars_from = self.f().locals_top;
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
                let body_locals_from = self.f().locals_top;
                let start = self.here();
                let mut j_end = None;
                if let Some(test) = &fs.test {
                    let m = self.mark();
                    let cond = self.expr_any(test)?;
                    j_end = Some(self.emit(Op::JumpIfFalse { cond, target: 0 }));
                    self.release(m);
                }
                self.push_loop(true, body_locals_from, loop_vars_from);
                self.statement(&fs.body)?;
                let cont = self.here();
                if has_lexical_init {
                    self.emit(Op::CloseUpvalues {
                        from_reg: loop_vars_from,
                    });
                }
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
            Statement::ForInStatement(fi) => {
                self.for_in_or_of(&fi.left, &fi.right, &fi.body, true)?;
            }
            Statement::ForOfStatement(fo) => {
                if fo.r#await {
                    return unsupported("for await..of");
                }
                self.for_in_or_of(&fo.left, &fo.right, &fo.body, false)?;
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
            Statement::ClassDeclaration(c) => self.class_decl(c)?,
            Statement::WithStatement(_) => return unsupported("with"),
            _ => return unsupported("declaração de módulo ou TypeScript"),
        }
        Ok(())
    }

    fn for_in_or_of(
        &mut self,
        left: &'a ForStatementLeft<'a>,
        right: &'a Expression<'a>,
        body: &'a Statement<'a>,
        is_for_in: bool,
    ) -> CResult<()> {
        let iter = self.alloc()?;
        let rhs = self.alloc()?;
        self.expr(right, rhs)?;
        self.emit(if is_for_in {
            Op::ForInInit { dst: iter, obj: rhs }
        } else {
            Op::ForOfInit { dst: iter, src: rhs }
        });

        self.push_scope();
        let has_lexical = matches!(
            left,
            ForStatementLeft::VariableDeclaration(d) if d.kind != VariableDeclarationKind::Var
        );
        let loop_vars_from = self.f().locals_top;

        // Pré-declara as variáveis léxicas do `for (let/const x ...)` no escopo do laço
        // antes do início para preservar registradores fixos entre iterações.
        if let ForStatementLeft::VariableDeclaration(d) = left
            && has_lexical
            && !self.at_script_top()
        {
            let mut names = Vec::new();
            for decl in &d.declarations {
                collect_pattern_names(&decl.id, &mut names);
            }
            for name in names {
                self.declare_local(name)?;
            }
        }
        let body_locals_from = self.f().locals_top;
        let elem = self.alloc()?;
        self.f().locals_top = self.f().locals_top.max(elem + 1);

        let start = self.here();
        let j_done = self.emit(Op::IterNext {
            iter,
            dst: elem,
            done_target: 0,
        });

        match left {
            ForStatementLeft::VariableDeclaration(d) => {
                let is_var = d.kind == VariableDeclarationKind::Var;
                let is_top = self.at_script_top();
                if let Some(decl) = d.declarations.first() {
                    self.bind_pattern(&decl.id, elem, is_var, is_top)?;
                }
            }
            target => {
                let Some(at) = target.as_assignment_target() else {
                    return unsupported("alvo de for-in/for-of");
                };
                self.assign_target(at, elem)?;
            }
        }

        self.push_loop(true, body_locals_from, loop_vars_from);
        self.statement(body)?;
        let cont = self.here();
        if has_lexical {
            self.emit(Op::CloseUpvalues {
                from_reg: loop_vars_from,
            });
        }
        self.emit(Op::Jump { target: start });
        let end = self.here();
        self.patch(j_done, end);
        self.pop_loop(cont, end);
        self.pop_scope();
        Ok(())
    }

    fn block(&mut self, body: &'a [Statement<'a>]) -> CResult<()> {
        self.push_scope();
        self.pre_declare_lexical(body)?;
        self.hoist_functions(body)?;
        for s in body {
            self.statement(s)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn var_decl(&mut self, d: &'a VariableDeclaration<'a>) -> CResult<()> {
        let is_var = d.kind == VariableDeclarationKind::Var;
        let is_top = self.at_script_top();
        for decl in &d.declarations {
            if let Some(init) = &decl.init {
                let val_reg = match &decl.id {
                    BindingPattern::BindingIdentifier(id) if !is_var && !is_top => {
                        self.declare_local(id.name.as_str())?
                    }
                    BindingPattern::BindingIdentifier(id) => match self.resolve(id.name.as_str())? {
                        Res::Local(r) => r,
                        _ => self.alloc()?,
                    },
                    _ => self.alloc()?,
                };
                self.expr(init, val_reg)?;
                self.bind_pattern(&decl.id, val_reg, is_var, is_top)?;
            } else if !is_var {
                let undef = self.alloc()?;
                self.emit(Op::LoadUndefined { dst: undef });
                self.bind_pattern(&decl.id, undef, false, is_top)?;
            }
        }
        Ok(())
    }

    fn bind_pattern(
        &mut self,
        pat: &'a BindingPattern<'a>,
        src: Reg,
        is_var: bool,
        is_top: bool,
    ) -> CResult<()> {
        match pat {
            BindingPattern::BindingIdentifier(id) => {
                let name = id.name.as_str();
                if !is_var && !is_top {
                    let r = self.declare_local(name)?;
                    if r != src {
                        self.emit(Op::Move { dst: r, src });
                    }
                } else {
                    if !is_var && is_top {
                        let n = self.str_const(name);
                        self.emit(Op::DeclareGlobal { name: n });
                    }
                    self.store_name(name, src)?;
                }
            }
            BindingPattern::AssignmentPattern(ap) => {
                let tmp = self.alloc()?;
                self.emit(Op::Move { dst: tmp, src });
                let skip = self.emit(Op::JumpIfNotUndefined {
                    cond: tmp,
                    target: 0,
                });
                self.expr(&ap.right, tmp)?;
                self.patch_here(skip);
                self.bind_pattern(&ap.left, tmp, is_var, is_top)?;
            }
            BindingPattern::ArrayPattern(ap) => {
                let iter = self.alloc()?;
                self.emit(Op::ForOfInit { dst: iter, src });
                for el in &ap.elements {
                    let m = self.mark();
                    let val = self.alloc()?;
                    let j = self.emit(Op::IterNext {
                        iter,
                        dst: val,
                        done_target: 0,
                    });
                    self.patch_here(j);
                    if let Some(sub) = el {
                        self.bind_pattern(sub, val, is_var, is_top)?;
                    }
                    self.release(m);
                }
                if let Some(rest) = &ap.rest {
                    let m = self.mark();
                    let rest_arr = self.alloc()?;
                    self.emit(Op::IterRest {
                        iter,
                        dst: rest_arr,
                    });
                    self.bind_pattern(&rest.argument, rest_arr, is_var, is_top)?;
                    self.release(m);
                }
            }
            BindingPattern::ObjectPattern(op) => {
                self.emit(Op::CheckObjectNotNullish { src });
                let has_rest = op.rest.is_some();
                let mut excl_regs = Vec::new();
                for prop in &op.properties {
                    let m = self.mark();
                    let val = self.alloc()?;
                    if let Some(k_reg) =
                        self.get_property_with_key(src, &prop.key, prop.computed, val, has_rest)?
                    {
                        excl_regs.push(k_reg);
                    }
                    self.bind_pattern(&prop.value, val, is_var, is_top)?;
                    if !has_rest {
                        self.release(m);
                    }
                }
                if let Some(rest) = &op.rest {
                    let rest_obj = self.alloc()?;
                    let excl_start = self.mark();
                    let mut consecutive = Vec::with_capacity(excl_regs.len());
                    for _ in &excl_regs {
                        consecutive.push(self.alloc()?);
                    }
                    for (dst_k, src_k) in consecutive.into_iter().zip(excl_regs) {
                        if dst_k != src_k {
                            self.emit(Op::Move {
                                dst: dst_k,
                                src: src_k,
                            });
                        }
                    }
                    let excl_count = u16::try_from(op.properties.len())
                        .map_err(|_| JsError::Unsupported("chaves demais no rest".into()))?;
                    self.emit(Op::ObjectRest {
                        dst: rest_obj,
                        obj: src,
                        excl_start,
                        excl_count,
                    });
                    self.bind_pattern(&rest.argument, rest_obj, is_var, is_top)?;
                }
            }
        }
        Ok(())
    }

    fn get_property_with_key(
        &mut self,
        obj: Reg,
        key: &'a PropertyKey<'a>,
        computed: bool,
        dst: Reg,
        save_key: bool,
    ) -> CResult<Option<Reg>> {
        let static_key = if computed {
            None
        } else {
            match key {
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
            Some(k) => {
                let name = self.str_const(&k);
                self.emit(Op::GetNamed { dst, obj, name });
                if save_key {
                    let kr = self.alloc()?;
                    self.emit(Op::LoadConst { dst: kr, idx: name });
                    Ok(Some(kr))
                } else {
                    Ok(None)
                }
            }
            None => {
                let Some(ke) = key.as_expression() else {
                    return unsupported("chave de propriedade");
                };
                let kr = self.alloc()?;
                self.expr(ke, kr)?;
                self.emit(Op::GetKeyed { dst, obj, key: kr });
                Ok(save_key.then_some(kr))
            }
        }
    }

    fn set_property_key(
        &mut self,
        obj: Reg,
        key: &'a PropertyKey<'a>,
        computed: bool,
        src: Reg,
    ) -> CResult<()> {
        let static_key = if computed {
            None
        } else {
            match key {
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
            Some(k) => {
                let name = self.str_const(&k);
                self.emit(Op::SetNamed { obj, name, src });
            }
            None => {
                let Some(ke) = key.as_expression() else {
                    return unsupported("chave de propriedade");
                };
                let kr = self.alloc()?;
                self.expr(ke, kr)?;
                self.emit(Op::SetKeyed { obj, key: kr, src });
            }
        }
        Ok(())
    }

    fn push_loop(
        &mut self,
        accepts_continue: bool,
        continue_locals_from: u16,
        break_locals_from: u16,
    ) {
        let f = self.f();
        let try_depth = f.try_depth;
        f.loops.push(LoopCtx {
            breaks: Vec::new(),
            continues: Vec::new(),
            try_depth,
            continue_locals_from,
            break_locals_from,
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
        let (pops, from_reg, idx) = {
            let f = self.f();
            let Some(idx) = f
                .loops
                .iter()
                .rposition(|l| !is_continue || l.accepts_continue)
            else {
                return Err(JsError::Syntax("break/continue fora de laço".into()));
            };
            let pops = f.try_depth - f.loops[idx].try_depth;
            let from_reg = if is_continue {
                f.loops[idx].continue_locals_from
            } else {
                f.loops[idx].break_locals_from
            };
            (pops, from_reg, idx)
        };
        for _ in 0..pops {
            self.emit(Op::ExitTry);
        }
        if self.f().locals_top > from_reg {
            self.emit(Op::CloseUpvalues { from_reg });
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

    fn switch(&mut self, sw: &'a SwitchStatement<'a>) -> CResult<()> {
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
        let ltop = self.f().locals_top;
        self.push_loop(false, ltop, ltop);
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
            self.pre_declare_lexical(&case.consequent)?;
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

    fn try_statement(&mut self, t: &'a TryStatement<'a>) -> CResult<()> {
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
                    self.bind_pattern(&param.pattern, exc, false, false)?;
                }
                self.pre_declare_lexical(&handler.body.body)?;
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
                if let Some(fin) = &t.finalizer {
                    self.block(&fin.body)?;
                }
                self.emit(Op::Throw { src: exc });
            }
        }
        self.patch_here(j_end);
        Ok(())
    }

    // ------------------------------------------------------------ classes

    fn class_decl(&mut self, cls: &'a Class<'a>) -> CResult<()> {
        let Some(id) = &cls.id else {
            return Err(JsError::Syntax("declaração de classe precisa de nome".into()));
        };
        let name = id.name.as_str();
        if self.at_script_top() {
            let m = self.mark();
            let t = self.alloc()?;
            self.compile_class(cls, t)?;
            let n = self.str_const(name);
            self.emit(Op::DeclareGlobal { name: n });
            self.emit(Op::SetGlobal { name: n, src: t });
            self.release(m);
        } else {
            let r = self.declare_local(name)?;
            self.compile_class(cls, r)?;
        }
        Ok(())
    }

    fn compile_class(&mut self, cls: &'a Class<'a>, dst: Reg) -> CResult<()> {
        let class_name = cls.id.as_ref().map(|i| i.name.as_str()).unwrap_or("");
        let has_super = cls.heritage.is_some();
        let super_reg = self.alloc()?;
        if let Some(heritage) = &cls.heritage {
            self.expr(&heritage.expression, super_reg)?;
        } else {
            self.emit(Op::LoadUndefined { dst: super_reg });
        }

        let mut ctor_method: Option<&MethodDefinition<'a>> = None;
        let mut instance_fields: Vec<&'a PropertyDefinition<'a>> = Vec::new();

        for el in &cls.body.body {
            match el {
                ClassElement::MethodDefinition(m) => {
                    if m.kind == MethodDefinitionKind::Constructor {
                        ctor_method = Some(m);
                    } else if matches!(m.kind, MethodDefinitionKind::Get | MethodDefinitionKind::Set)
                    {
                        return unsupported("getter/setter em classe");
                    }
                }
                ClassElement::PropertyDefinition(p) if !p.r#static => {
                    instance_fields.push(p);
                }
                ClassElement::PropertyDefinition(_) | ClassElement::StaticBlock(_) => {}
                _ => return unsupported("elemento de classe"),
            }
        }

        // Em ClassExpression nomeada, o nome interno é visível dentro dos métodos/ctor.
        let pushed_expr_name =
            cls.r#type == ClassType::ClassExpression && !class_name.is_empty();
        let inner_name_reg = if pushed_expr_name {
            self.push_scope();
            Some(self.declare_local(class_name)?)
        } else {
            None
        };

        // Compila o construtor
        let ctor_depth = self.fns.len();
        self.pending_fields.push((ctor_depth, instance_fields));
        let ctor_idx = if let Some(ctor) = ctor_method {
            let func = &ctor.value;
            if func.generator || func.r#async {
                self.pending_fields.pop();
                return unsupported("constructor async/generator");
            }
            self.function(
                class_name,
                &func.params,
                func.body.as_deref(),
                None,
                false,
                true,
                has_super,
            )?
        } else {
            self.compile_default_ctor(class_name, has_super)?
        };
        self.pending_fields.pop();

        self.emit(Op::Closure {
            dst,
            func: ctor_idx,
        });
        if let Some(ir) = inner_name_reg {
            self.emit(Op::Move { dst: ir, src: dst });
        }
        self.emit(Op::SetupClass {
            ctor: dst,
            super_ctor: super_reg,
        });

        let proto_reg = self.alloc()?;
        let k_proto = self.str_const("prototype");
        self.emit(Op::GetNamed {
            dst: proto_reg,
            obj: dst,
            name: k_proto,
        });

        // Instala métodos, propriedades estáticas e blocos estáticos
        for el in &cls.body.body {
            match el {
                ClassElement::MethodDefinition(m)
                    if m.kind != MethodDefinitionKind::Constructor =>
                {
                    let target_obj = if m.r#static { dst } else { proto_reg };
                    let m_name = match &m.key {
                        PropertyKey::StaticIdentifier(id) if !m.computed => id.name.as_str(),
                        _ => "",
                    };
                    let idx = self.function_decl(&m.value, m_name)?;
                    let fn_reg = self.alloc()?;
                    self.emit(Op::Closure {
                        dst: fn_reg,
                        func: idx,
                    });
                    self.define_method_key(target_obj, &m.key, m.computed, fn_reg, false)?;
                }
                ClassElement::PropertyDefinition(p) if p.r#static => {
                    let val_reg = self.alloc()?;
                    if let Some(init) = &p.value {
                        self.expr(init, val_reg)?;
                    } else {
                        self.emit(Op::LoadUndefined { dst: val_reg });
                    }
                    self.set_property_key(dst, &p.key, p.computed, val_reg)?;
                }
                ClassElement::StaticBlock(sb) => {
                    self.fns.push(FnState::new(
                        "<static>", 0, 0, false, false, false, false, false,
                    ));
                    let mut vars = Vec::new();
                    sb.body.iter().for_each(|s| collect_vars(s, &mut vars));
                    for v in vars {
                        self.declare_local(v)?;
                    }
                    self.pre_declare_lexical(&sb.body)?;
                    self.hoist_functions(&sb.body)?;
                    for s in &sb.body {
                        self.statement(s)?;
                    }
                    let t = self.alloc()?;
                    self.emit(Op::LoadUndefined { dst: t });
                    self.emit(Op::Return { src: t });
                    let st = self.fns.pop().expect("static block");
                    let code = Rc::new(st.finish());
                    self.f().functions.push(code);
                    let idx = (self.f().functions.len() - 1) as u32;
                    let blk_fn = self.alloc()?;
                    self.emit(Op::Closure {
                        dst: blk_fn,
                        func: idx,
                    });
                    let dummy = self.alloc()?;
                    let args_start = self.mark();
                    self.emit(Op::Call {
                        dst: dummy,
                        callee: blk_fn,
                        this: dst,
                        args: args_start,
                        argc: 0,
                    });
                }
                _ => {}
            }
        }

        if pushed_expr_name {
            self.pop_scope();
        }
        Ok(())
    }

    fn compile_default_ctor(&mut self, class_name: &str, has_super: bool) -> CResult<u32> {
        self.fns.push(FnState::new(
            class_name,
            0,
            0,
            has_super,
            false,
            true,
            has_super,
            false,
        ));
        if has_super {
            let args_reg = self.alloc()?; // registrador 0 = ...args
            self.f().locals_top = self.f().next_reg;
            let super_res = self.alloc()?;
            self.emit(Op::SuperCallSpread {
                dst: super_res,
                args_array: args_reg,
            });
            self.emit(Op::FinishSuper { src: super_res });
            self.emit_pending_fields_for_current_fn()?;
            self.emit(Op::Return { src: super_res });
        } else {
            self.emit_pending_fields_for_current_fn()?;
            let t = self.alloc()?;
            self.emit(Op::LoadUndefined { dst: t });
            self.emit(Op::Return { src: t });
        }
        let state = self.fns.pop().expect("default ctor");
        let code = Rc::new(state.finish());
        let f = self.f();
        f.functions.push(code);
        Ok((f.functions.len() - 1) as u32)
    }

    fn define_method_key(
        &mut self,
        obj: Reg,
        key: &'a PropertyKey<'a>,
        computed: bool,
        func: Reg,
        enumerable: bool,
    ) -> CResult<()> {
        let static_key = if computed {
            None
        } else {
            match key {
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
            Some(k) => {
                let name = self.str_const(&k);
                self.emit(Op::DefineMethod {
                    obj,
                    name,
                    func,
                    enumerable,
                });
            }
            None => {
                let Some(ke) = key.as_expression() else {
                    return unsupported("chave de propriedade");
                };
                let kr = self.alloc()?;
                self.expr(ke, kr)?;
                self.emit(Op::DefineMethodKeyed {
                    obj,
                    key: kr,
                    func,
                    enumerable,
                });
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------- expressões

    /// Avalia `e` e devolve um registrador com o resultado (sem cópia se `e` for local).
    fn expr_any(&mut self, e: &'a Expression<'a>) -> CResult<Reg> {
        if let Expression::Identifier(id) = e
            && let Res::Local(r) = self.resolve(id.name.as_str())?
        {
            return Ok(r);
        }
        let t = self.alloc()?;
        self.expr(e, t)?;
        Ok(t)
    }

    /// Avalia `e` deixando o resultado em `dst`.
    fn expr(&mut self, e: &'a Expression<'a>, dst: Reg) -> CResult<()> {
        let m = self.mark();
        self.expr_inner(e, dst)?;
        self.release(m);
        Ok(())
    }

    fn expr_inner(&mut self, e: &'a Expression<'a>, dst: Reg) -> CResult<()> {
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
                    match el {
                        ArrayExpressionElement::SpreadElement(sp) => {
                            let r = self.expr_any(&sp.argument)?;
                            self.emit(Op::ArrayPushSpread {
                                array: dst,
                                iterable: r,
                            });
                        }
                        ArrayExpressionElement::Elision(_) => {
                            let t = self.alloc()?;
                            self.emit(Op::LoadUndefined { dst: t });
                            self.emit(Op::ArrayPush { array: dst, src: t });
                        }
                        other => {
                            let e = other.as_expression().expect("elemento é expressão");
                            let r = self.expr_any(e)?;
                            self.emit(Op::ArrayPush { array: dst, src: r });
                        }
                    }
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
                        self.function("", &a.params, Some(&**b), None, true, false, false)?
                    }
                    other => {
                        let e = other.as_expression().expect("corpo de arrow é expressão");
                        self.function("", &a.params, None, Some(e), true, false, false)?
                    }
                };
                self.emit(Op::Closure { dst, func: idx });
            }
            Expression::ClassExpression(cls) => self.compile_class(cls, dst)?,
            Expression::UnaryExpression(u) => self.unary(u, dst)?,
            Expression::UpdateExpression(u) => self.update(u, dst)?,
            Expression::BinaryExpression(b) => {
                let lhs = if is_simple(&b.right) {
                    self.expr_any(&b.left)?
                } else {
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
                if has_spread(&n.arguments) {
                    let args_array = self.arguments_array(&n.arguments)?;
                    self.emit(Op::NewSpread {
                        dst,
                        callee,
                        args_array,
                    });
                } else {
                    let (args, argc) = self.arguments(&n.arguments)?;
                    self.emit(Op::New {
                        dst,
                        callee,
                        args,
                        argc,
                    });
                }
            }
            Expression::StaticMemberExpression(me) => {
                if me.optional {
                    return unsupported("encadeamento opcional sem ChainExpression");
                }
                let name = self.str_const(me.property.name.as_str());
                if matches!(&me.object, Expression::Super(_)) {
                    self.emit(Op::GetSuperNamed { dst, name });
                } else {
                    let obj = self.expr_any(&me.object)?;
                    self.emit(Op::GetNamed { dst, obj, name });
                }
            }
            Expression::ComputedMemberExpression(me) => {
                if me.optional {
                    return unsupported("encadeamento opcional sem ChainExpression");
                }
                if matches!(&me.object, Expression::Super(_)) {
                    let key = self.expr_any(&me.expression)?;
                    self.emit(Op::GetSuperKeyed { dst, key });
                } else {
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
            }
            Expression::ChainExpression(ce) => self.chain(ce, dst)?,
            Expression::PrivateFieldExpression(_) => return unsupported("campos privados (#x)"),
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

    fn chain(&mut self, ce: &'a ChainExpression<'a>, dst: Reg) -> CResult<()> {
        let mut shorts = Vec::new();
        match &ce.expression {
            ChainElement::CallExpression(c) => {
                self.compile_chain_call(c, dst, &mut shorts)?;
            }
            ChainElement::StaticMemberExpression(me) => {
                self.compile_chain_static(me, dst, &mut shorts)?;
            }
            ChainElement::ComputedMemberExpression(me) => {
                self.compile_chain_computed(me, dst, &mut shorts)?;
            }
            _ => return unsupported("encadeamento opcional em campo privado"),
        }
        if !shorts.is_empty() {
            let j_end = self.emit(Op::Jump { target: 0 });
            for s in shorts {
                self.patch_here(s);
            }
            self.emit(Op::LoadUndefined { dst });
            self.patch_here(j_end);
        }
        Ok(())
    }

    fn compile_chain_expr(
        &mut self,
        e: &'a Expression<'a>,
        dst: Reg,
        shorts: &mut Vec<usize>,
    ) -> CResult<()> {
        match e {
            Expression::StaticMemberExpression(me) => self.compile_chain_static(me, dst, shorts),
            Expression::ComputedMemberExpression(me) => {
                self.compile_chain_computed(me, dst, shorts)
            }
            Expression::CallExpression(c) => self.compile_chain_call(c, dst, shorts),
            other => self.expr(other, dst),
        }
    }

    fn compile_chain_static(
        &mut self,
        me: &'a StaticMemberExpression<'a>,
        dst: Reg,
        shorts: &mut Vec<usize>,
    ) -> CResult<()> {
        let obj = self.alloc()?;
        self.compile_chain_expr(&me.object, obj, shorts)?;
        if me.optional {
            shorts.push(self.emit(Op::JumpIfNullish {
                cond: obj,
                target: 0,
            }));
        }
        let name = self.str_const(me.property.name.as_str());
        self.emit(Op::GetNamed { dst, obj, name });
        Ok(())
    }

    fn compile_chain_computed(
        &mut self,
        me: &'a ComputedMemberExpression<'a>,
        dst: Reg,
        shorts: &mut Vec<usize>,
    ) -> CResult<()> {
        let obj = self.alloc()?;
        self.compile_chain_expr(&me.object, obj, shorts)?;
        if me.optional {
            shorts.push(self.emit(Op::JumpIfNullish {
                cond: obj,
                target: 0,
            }));
        }
        let key = self.expr_any(&me.expression)?;
        self.emit(Op::GetKeyed { dst, obj, key });
        Ok(())
    }

    fn compile_chain_call(
        &mut self,
        c: &'a CallExpression<'a>,
        dst: Reg,
        shorts: &mut Vec<usize>,
    ) -> CResult<()> {
        let callee = self.alloc()?;
        let this = self.alloc()?;
        match &c.callee {
            Expression::StaticMemberExpression(me) => {
                self.compile_chain_expr(&me.object, this, shorts)?;
                if me.optional {
                    shorts.push(self.emit(Op::JumpIfNullish {
                        cond: this,
                        target: 0,
                    }));
                }
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::GetNamed {
                    dst: callee,
                    obj: this,
                    name,
                });
            }
            Expression::ComputedMemberExpression(me) => {
                self.compile_chain_expr(&me.object, this, shorts)?;
                if me.optional {
                    shorts.push(self.emit(Op::JumpIfNullish {
                        cond: this,
                        target: 0,
                    }));
                }
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
                self.compile_chain_expr(other, callee, shorts)?;
                self.emit(Op::LoadUndefined { dst: this });
            }
        }
        if c.optional {
            shorts.push(self.emit(Op::JumpIfNullish {
                cond: callee,
                target: 0,
            }));
        }
        if has_spread(&c.arguments) {
            let args_array = self.arguments_array(&c.arguments)?;
            self.emit(Op::CallSpread {
                dst,
                callee,
                this,
                args_array,
            });
        } else {
            let (args, argc) = self.arguments(&c.arguments)?;
            self.emit(Op::Call {
                dst,
                callee,
                this,
                args,
                argc,
            });
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

    fn template(&mut self, t: &'a TemplateLiteral<'a>, dst: Reg) -> CResult<()> {
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

    fn object(&mut self, o: &'a ObjectExpression<'a>, dst: Reg) -> CResult<()> {
        self.emit(Op::NewObject { dst });
        for prop in &o.properties {
            let m = self.mark();
            match prop {
                ObjectPropertyKind::SpreadProperty(sp) => {
                    let src = self.expr_any(&sp.argument)?;
                    self.emit(Op::ObjectAssignSpread { obj: dst, src });
                }
                ObjectPropertyKind::ObjectProperty(p) => {
                    if p.kind != PropertyKind::Init {
                        return unsupported("getter/setter em objeto literal");
                    }
                    let v = self.expr_any(&p.value)?;
                    if p.method {
                        self.define_method_key(dst, &p.key, p.computed, v, true)?;
                    } else {
                        self.set_property_key(dst, &p.key, p.computed, v)?;
                    }
                }
            }
            self.release(m);
        }
        Ok(())
    }

    fn unary(&mut self, u: &'a UnaryExpression<'a>, dst: Reg) -> CResult<()> {
        if u.operator == UnaryOperator::Delete {
            return self.compile_delete(&u.argument, dst);
        }
        let op = match u.operator {
            UnaryOperator::UnaryNegation => UnOp::Neg,
            UnaryOperator::UnaryPlus => UnOp::Plus,
            UnaryOperator::LogicalNot => UnOp::Not,
            UnaryOperator::BitwiseNot => UnOp::BitNot,
            UnaryOperator::Void => UnOp::Void,
            UnaryOperator::Typeof => {
                if let Expression::Identifier(id) = &u.argument
                    && let Res::Global = self.resolve(id.name.as_str())?
                {
                    let name = self.str_const(id.name.as_str());
                    self.emit(Op::TypeOfGlobal { dst, name });
                    return Ok(());
                }
                UnOp::TypeOf
            }
            UnaryOperator::Delete => unreachable!(),
        };
        let src = self.expr_any(&u.argument)?;
        self.emit(Op::Unary { op, dst, src });
        Ok(())
    }

    fn compile_delete(&mut self, arg: &'a Expression<'a>, dst: Reg) -> CResult<()> {
        match arg {
            Expression::ParenthesizedExpression(p) => self.compile_delete(&p.expression, dst),
            Expression::StaticMemberExpression(me) => {
                let obj = self.expr_any(&me.object)?;
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::DeleteNamed { dst, obj, name });
                Ok(())
            }
            Expression::ComputedMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let key = self.expr_any(&me.expression)?;
                self.emit(Op::DeleteKeyed { dst, obj, key });
                Ok(())
            }
            Expression::Identifier(id) => match self.resolve(id.name.as_str())? {
                Res::Global => {
                    let name = self.str_const(id.name.as_str());
                    self.emit(Op::DeleteGlobal { dst, name });
                    Ok(())
                }
                _ => {
                    self.emit(Op::LoadBool { dst, value: false });
                    Ok(())
                }
            },
            other => {
                self.expr_any(other)?;
                self.emit(Op::LoadBool { dst, value: true });
                Ok(())
            }
        }
    }

    fn update(&mut self, u: &'a UpdateExpression<'a>, dst: Reg) -> CResult<()> {
        let op = match u.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let old = self.alloc()?;
        let new = self.alloc()?;
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

    fn assign(&mut self, a: &'a AssignmentExpression<'a>, dst: Reg) -> CResult<()> {
        let op = a.operator;
        let plain = op == AssignmentOperator::Assign;
        match &a.left {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if plain {
                    match self.resolve(name)? {
                        Res::Local(r) => {
                            self.expr(&a.right, r)?;
                            if r != dst {
                                self.emit(Op::Move { dst, src: r });
                            }
                        }
                        Res::Upvalue(uv) => {
                            self.expr(&a.right, dst)?;
                            self.emit(Op::SetUpvalue {
                                upvalue: uv,
                                src: dst,
                            });
                        }
                        Res::Global => {
                            self.expr(&a.right, dst)?;
                            self.store_name(name, dst)?;
                        }
                    }
                    return Ok(());
                }
                self.load_name(name, dst)?;
                self.compound(op, dst, &a.right)?;
                self.store_name(name, dst)?;
            }
            AssignmentTarget::StaticMemberExpression(me) => {
                let obj = self.alloc()?;
                if matches!(&me.object, Expression::Super(_)) {
                    self.emit(Op::LoadThis { dst: obj });
                } else {
                    self.expr(&me.object, obj)?;
                }
                let name = self.str_const(me.property.name.as_str());
                if plain {
                    self.expr(&a.right, dst)?;
                } else {
                    if matches!(&me.object, Expression::Super(_)) {
                        self.emit(Op::GetSuperNamed { dst, name });
                    } else {
                        self.emit(Op::GetNamed { dst, obj, name });
                    }
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
                if matches!(&me.object, Expression::Super(_)) {
                    self.emit(Op::LoadThis { dst: obj });
                } else {
                    self.expr(&me.object, obj)?;
                }
                let key = self.alloc()?;
                self.expr(&me.expression, key)?;
                if plain {
                    self.expr(&a.right, dst)?;
                } else {
                    if matches!(&me.object, Expression::Super(_)) {
                        self.emit(Op::GetSuperKeyed { dst, key });
                    } else {
                        self.emit(Op::GetKeyed { dst, obj, key });
                    }
                    self.compound(op, dst, &a.right)?;
                }
                self.emit(Op::SetKeyed { obj, key, src: dst });
            }
            pat => {
                if !plain {
                    return unsupported("operador composto com desestruturação");
                }
                self.expr(&a.right, dst)?;
                self.assign_target(pat, dst)?;
            }
        }
        Ok(())
    }

    fn assign_target(&mut self, target: &'a AssignmentTarget<'a>, src: Reg) -> CResult<()> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.store_name(id.name.as_str(), src)
            }
            AssignmentTarget::StaticMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let name = self.str_const(me.property.name.as_str());
                self.emit(Op::SetNamed { obj, name, src });
                Ok(())
            }
            AssignmentTarget::ComputedMemberExpression(me) => {
                let obj = self.alloc()?;
                self.expr(&me.object, obj)?;
                let key = self.alloc()?;
                self.expr(&me.expression, key)?;
                self.emit(Op::SetKeyed { obj, key, src });
                Ok(())
            }
            AssignmentTarget::ArrayAssignmentTarget(at) => {
                let iter = self.alloc()?;
                self.emit(Op::ForOfInit { dst: iter, src });
                for el in &at.elements {
                    let m = self.mark();
                    let val = self.alloc()?;
                    let j = self.emit(Op::IterNext {
                        iter,
                        dst: val,
                        done_target: 0,
                    });
                    self.patch_here(j);
                    if let Some(sub) = el {
                        self.assign_maybe_default(sub, val)?;
                    }
                    self.release(m);
                }
                if let Some(rest) = &at.rest {
                    let m = self.mark();
                    let rest_arr = self.alloc()?;
                    self.emit(Op::IterRest {
                        iter,
                        dst: rest_arr,
                    });
                    self.assign_target(&rest.target, rest_arr)?;
                    self.release(m);
                }
                Ok(())
            }
            AssignmentTarget::ObjectAssignmentTarget(ot) => {
                self.emit(Op::CheckObjectNotNullish { src });
                let has_rest = ot.rest.is_some();
                let mut excl_regs = Vec::new();
                for prop in &ot.properties {
                    let m = self.mark();
                    let val = self.alloc()?;
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(pi) => {
                            let name_str = pi.binding.name.as_str();
                            let name = self.str_const(name_str);
                            self.emit(Op::GetNamed {
                                dst: val,
                                obj: src,
                                name,
                            });
                            if has_rest {
                                let kr = self.alloc()?;
                                self.emit(Op::LoadConst { dst: kr, idx: name });
                                excl_regs.push(kr);
                            }
                            if let Some(init) = &pi.init {
                                let skip = self.emit(Op::JumpIfNotUndefined {
                                    cond: val,
                                    target: 0,
                                });
                                self.expr(init, val)?;
                                self.patch_here(skip);
                            }
                            self.store_name(name_str, val)?;
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(pp) => {
                            if let Some(kr) = self.get_property_with_key(
                                src,
                                &pp.name,
                                pp.computed,
                                val,
                                has_rest,
                            )? {
                                excl_regs.push(kr);
                            }
                            self.assign_maybe_default(&pp.binding, val)?;
                        }
                    }
                    if !has_rest {
                        self.release(m);
                    }
                }
                if let Some(rest) = &ot.rest {
                    let rest_obj = self.alloc()?;
                    let excl_start = self.mark();
                    let mut consecutive = Vec::with_capacity(excl_regs.len());
                    for _ in &excl_regs {
                        consecutive.push(self.alloc()?);
                    }
                    for (dst_k, src_k) in consecutive.into_iter().zip(excl_regs) {
                        if dst_k != src_k {
                            self.emit(Op::Move {
                                dst: dst_k,
                                src: src_k,
                            });
                        }
                    }
                    let excl_count = u16::try_from(ot.properties.len())
                        .map_err(|_| JsError::Unsupported("chaves demais no rest".into()))?;
                    self.emit(Op::ObjectRest {
                        dst: rest_obj,
                        obj: src,
                        excl_start,
                        excl_count,
                    });
                    self.assign_target(&rest.target, rest_obj)?;
                }
                Ok(())
            }
            _ => unsupported("alvo de atribuição não suportado"),
        }
    }

    fn assign_maybe_default(
        &mut self,
        target: &'a AssignmentTargetMaybeDefault<'a>,
        src: Reg,
    ) -> CResult<()> {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(awd) => {
                let tmp = self.alloc()?;
                self.emit(Op::Move { dst: tmp, src });
                let skip = self.emit(Op::JumpIfNotUndefined {
                    cond: tmp,
                    target: 0,
                });
                self.expr(&awd.init, tmp)?;
                self.patch_here(skip);
                self.assign_target(&awd.binding, tmp)
            }
            _ => {
                let at = target.to_assignment_target();
                self.assign_target(at, src)
            }
        }
    }

    fn compound(
        &mut self,
        op: AssignmentOperator,
        dst: Reg,
        right: &'a Expression<'a>,
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

    fn call(&mut self, c: &'a CallExpression<'a>, dst: Reg) -> CResult<()> {
        if c.optional {
            return unsupported("chamada opcional sem ChainExpression");
        }
        if matches!(&c.callee, Expression::Super(_)) {
            if has_spread(&c.arguments) {
                let args_array = self.arguments_array(&c.arguments)?;
                self.emit(Op::SuperCallSpread { dst, args_array });
            } else {
                let (args, argc) = self.arguments(&c.arguments)?;
                self.emit(Op::SuperCall { dst, args, argc });
            }
            self.emit(Op::FinishSuper { src: dst });
            self.emit_pending_fields_for_current_fn()?;
            return Ok(());
        }
        let callee = self.alloc()?;
        let this = self.alloc()?;
        match &c.callee {
            Expression::StaticMemberExpression(me) if !me.optional => {
                let name = self.str_const(me.property.name.as_str());
                if matches!(&me.object, Expression::Super(_)) {
                    self.emit(Op::LoadThis { dst: this });
                    self.emit(Op::GetSuperNamed { dst: callee, name });
                } else {
                    self.expr(&me.object, this)?;
                    self.emit(Op::GetNamed {
                        dst: callee,
                        obj: this,
                        name,
                    });
                }
            }
            Expression::ComputedMemberExpression(me) if !me.optional => {
                if matches!(&me.object, Expression::Super(_)) {
                    self.emit(Op::LoadThis { dst: this });
                    let m = self.mark();
                    let key = self.expr_any(&me.expression)?;
                    self.emit(Op::GetSuperKeyed { dst: callee, key });
                    self.release(m);
                } else {
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
            }
            other => {
                self.expr(other, callee)?;
                self.emit(Op::LoadUndefined { dst: this });
            }
        }
        if has_spread(&c.arguments) {
            let args_array = self.arguments_array(&c.arguments)?;
            self.emit(Op::CallSpread {
                dst,
                callee,
                this,
                args_array,
            });
        } else {
            let (args, argc) = self.arguments(&c.arguments)?;
            self.emit(Op::Call {
                dst,
                callee,
                this,
                args,
                argc,
            });
        }
        Ok(())
    }

    fn arguments(&mut self, list: &'a [Argument<'a>]) -> CResult<(Reg, u16)> {
        let start = self.mark();
        let mut regs = Vec::with_capacity(list.len());
        for _ in list {
            regs.push(self.alloc()?);
        }
        for (a, r) in list.iter().zip(regs) {
            let e = a.as_expression().expect("argumento sem spread");
            self.expr(e, r)?;
        }
        let argc = u16::try_from(list.len())
            .map_err(|_| JsError::Unsupported("argumentos demais".into()))?;
        Ok((start, argc))
    }

    fn arguments_array(&mut self, list: &'a [Argument<'a>]) -> CResult<Reg> {
        let arr = self.alloc()?;
        self.emit(Op::NewArray { dst: arr });
        for a in list {
            let m = self.mark();
            match a {
                Argument::SpreadElement(sp) => {
                    let r = self.expr_any(&sp.argument)?;
                    self.emit(Op::ArrayPushSpread {
                        array: arr,
                        iterable: r,
                    });
                }
                other => {
                    let e = other.as_expression().expect("argumento é expressão");
                    let r = self.expr_any(e)?;
                    self.emit(Op::ArrayPush { array: arr, src: r });
                }
            }
            self.release(m);
        }
        Ok(arr)
    }
}

fn has_spread(list: &[Argument<'_>]) -> bool {
    list.iter()
        .any(|a| matches!(a, Argument::SpreadElement(_)))
}
