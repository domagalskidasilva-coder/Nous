//! Interpretador do bytecode.
//!
//! Chamadas entre funções JavaScript não usam a pilha do Rust: cada chamada empilha um
//! `Frame` e o mesmo laço continua. Só quando o Rust chama JavaScript (funções nativas,
//! conversões) é que um laço aninhado roda; nesses trechos o GC fica bloqueado, porque
//! pode haver valores vivos guardados em variáveis do Rust.

use std::rc::Rc;

use crate::bytecode::{BinOp, FunctionCode, Op, Reg, UnOp};
use crate::heap::{GcRef, Heap, ObjectKind};
use crate::value::{Kind, Value};

pub type NativeFn = fn(&mut Vm, Value, &[Value]) -> Result<Value, Value>;

const MAX_FRAMES: usize = 10_000;

pub(crate) struct Names {
    pub length: GcRef,
    pub prototype: GcRef,
    pub constructor: GcRef,
    pub message: GcRef,
    pub name: GcRef,
    pub to_string: GcRef,
    pub value_of: GcRef,
}

pub(crate) struct Protos {
    pub object: GcRef,
    pub function: GcRef,
    pub array: GcRef,
    pub string: GcRef,
    pub number: GcRef,
    pub boolean: GcRef,
    pub error: GcRef,
    pub type_error: GcRef,
    pub reference_error: GcRef,
    pub range_error: GcRef,
    pub syntax_error: GcRef,
}

struct Frame {
    code: Rc<FunctionCode>,
    pc: usize,
    base: usize,
    this: Value,
    /// Registrador absoluto do chamador que recebe o retorno (`None` = chamada vinda do Rust).
    ret_dst: Option<usize>,
    /// Objeto criado por `new`, devolvido se a função não retornar outro objeto.
    construct: Option<GcRef>,
    handlers: Vec<(u32, Reg)>,
}

/// Erro devolvido para quem usa o motor pelo Rust.
#[derive(Debug, Clone, PartialEq)]
pub enum JsError {
    Syntax(String),
    Unsupported(String),
    Thrown(String),
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsError::Syntax(m) => write!(f, "SyntaxError: {m}"),
            JsError::Unsupported(m) => write!(f, "ainda não suportado: {m}"),
            JsError::Thrown(m) => write!(f, "Uncaught {m}"),
        }
    }
}

impl std::error::Error for JsError {}

pub struct Vm {
    pub heap: Heap,
    pub global: GcRef,
    regs: Vec<Value>,
    frames: Vec<Frame>,
    pub(crate) names: Names,
    pub(crate) protos: Protos,
    native_depth: u32,
    /// Limite de instruções (para o runner do test262 não travar em laços infinitos).
    pub step_limit: Option<u64>,
    steps: u64,
    pub output: Box<dyn FnMut(&str)>,
    pub(crate) rng_state: u64,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        let mut heap = Heap::default();
        let names = Names {
            length: heap.intern("length"),
            prototype: heap.intern("prototype"),
            constructor: heap.intern("constructor"),
            message: heap.intern("message"),
            name: heap.intern("name"),
            to_string: heap.intern("toString"),
            value_of: heap.intern("valueOf"),
        };
        let object = heap.alloc_object(None, ObjectKind::Ordinary);
        let mut proto = |kind| heap.alloc_object(Some(object), kind);
        let function = proto(ObjectKind::Ordinary);
        let array = proto(ObjectKind::Array(Vec::new()));
        let string = proto(ObjectKind::Ordinary);
        let number = proto(ObjectKind::Ordinary);
        let boolean = proto(ObjectKind::Ordinary);
        let error = proto(ObjectKind::Ordinary);
        let mut sub = |_: &str| heap.alloc_object(Some(error), ObjectKind::Ordinary);
        let type_error = sub("TypeError");
        let reference_error = sub("ReferenceError");
        let range_error = sub("RangeError");
        let syntax_error = sub("SyntaxError");
        let global = heap.alloc_object(Some(object), ObjectKind::Ordinary);
        let mut vm = Vm {
            heap,
            global,
            regs: Vec::with_capacity(256),
            frames: Vec::new(),
            names,
            protos: Protos {
                object,
                function,
                array,
                string,
                number,
                boolean,
                error,
                type_error,
                reference_error,
                range_error,
                syntax_error,
            },
            native_depth: 0,
            step_limit: None,
            steps: 0,
            output: Box::new(|s| println!("{s}")),
            rng_state: 0x9E37_79B9_7F4A_7C15,
        };
        crate::builtins::install(&mut vm);
        vm
    }

    // ------------------------------------------------------------ API pública

    /// Compila e roda um script. Devolve o valor da última expressão.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        let code = crate::compiler::compile_script(&mut self.heap, source)?;
        let depth = self.frames.len();
        let this = Value::object(self.global);
        self.steps = 0;
        if let Err(e) = self.push_frame(Rc::new(code), this, &[], None, None) {
            return Err(JsError::Thrown(self.display(e)));
        }
        match self.run(depth) {
            Ok(v) => Ok(v),
            Err(e) => {
                let msg = self.display(e);
                Err(JsError::Thrown(msg))
            }
        }
    }

    /// Texto para mostrar um valor (como o console faz).
    pub fn display(&mut self, v: Value) -> String {
        match v.kind() {
            Kind::String(r) => self.heap.string(r).to_owned(),
            Kind::Object(o)
                if matches!(self.heap.object(o).kind, ObjectKind::Error)
                    || self.is_error_like(o) =>
            {
                let name = self.get_property(v, self.names.name).ok();
                let msg = self.get_property(v, self.names.message).ok();
                let name = name
                    .map(|n| self.stringify_lossy(n))
                    .unwrap_or_else(|| "Error".into());
                match msg.map(|m| self.stringify_lossy(m)) {
                    Some(m) if !m.is_empty() => format!("{name}: {m}"),
                    _ => name,
                }
            }
            _ => self.stringify_lossy(v),
        }
    }

    fn is_error_like(&self, o: GcRef) -> bool {
        let mut cur = self.heap.object(o).proto;
        while let Some(p) = cur {
            if p == self.protos.error {
                return true;
            }
            cur = self.heap.object(p).proto;
        }
        false
    }

    fn stringify_lossy(&mut self, v: Value) -> String {
        match self.to_str(v) {
            Ok(s) => s.to_string(),
            Err(_) => "[objeto]".into(),
        }
    }

    pub fn gc(&mut self) {
        let mut roots: Vec<Value> = self.regs.clone();
        roots.push(Value::object(self.global));
        let p = &self.protos;
        for r in [
            p.object,
            p.function,
            p.array,
            p.string,
            p.number,
            p.boolean,
            p.error,
            p.type_error,
            p.reference_error,
            p.range_error,
            p.syntax_error,
        ] {
            roots.push(Value::object(r));
        }
        for f in &self.frames {
            roots.push(f.this);
            if let Some(c) = f.construct {
                roots.push(Value::object(c));
            }
            for c in &f.code.constants {
                roots.push(*c);
            }
        }
        self.heap.collect(roots);
    }

    fn maybe_gc(&mut self) {
        if self.native_depth == 0 && self.heap.should_collect() {
            self.gc();
        }
    }

    // ---------------------------------------------------------------- erros

    pub fn make_error(&mut self, proto: GcRef, msg: &str) -> Value {
        let o = self.heap.alloc_object(Some(proto), ObjectKind::Error);
        let m = self.heap.alloc_string(msg);
        let key = self.names.message;
        self.heap.object_mut(o).set_own(key, Value::string(m));
        Value::object(o)
    }

    pub fn type_error(&mut self, msg: impl AsRef<str>) -> Value {
        self.make_error(self.protos.type_error, msg.as_ref())
    }

    pub fn reference_error(&mut self, msg: impl AsRef<str>) -> Value {
        self.make_error(self.protos.reference_error, msg.as_ref())
    }

    pub fn range_error(&mut self, msg: impl AsRef<str>) -> Value {
        self.make_error(self.protos.range_error, msg.as_ref())
    }

    // --------------------------------------------------------- conversões

    pub fn to_boolean(&self, v: Value) -> bool {
        match v.kind() {
            Kind::Undefined | Kind::Null => false,
            Kind::Bool(b) => b,
            Kind::Number(n) => !(n == 0.0 || n.is_nan()),
            Kind::String(r) => !self.heap.string(r).is_empty(),
            Kind::Object(_) => true,
        }
    }

    pub fn is_callable(&self, v: Value) -> bool {
        v.as_object()
            .is_some_and(|o| self.heap.object(o).is_callable())
    }

    pub fn to_primitive(&mut self, v: Value, prefer_string: bool) -> Result<Value, Value> {
        if v.as_object().is_none() {
            return Ok(v);
        }
        let order = if prefer_string {
            [self.names.to_string, self.names.value_of]
        } else {
            [self.names.value_of, self.names.to_string]
        };
        for key in order {
            let f = self.get_property(v, key)?;
            if self.is_callable(f) {
                let r = self.call_function(f, v, &[])?;
                if r.as_object().is_none() {
                    return Ok(r);
                }
            }
        }
        Err(self.type_error("Cannot convert object to primitive value"))
    }

    pub fn to_number(&mut self, v: Value) -> Result<f64, Value> {
        Ok(match v.kind() {
            Kind::Undefined => f64::NAN,
            Kind::Null => 0.0,
            Kind::Bool(b) => b as u8 as f64,
            Kind::Number(n) => n,
            Kind::String(r) => string_to_number(self.heap.string(r)),
            Kind::Object(_) => {
                let p = self.to_primitive(v, false)?;
                return self.to_number(p);
            }
        })
    }

    pub fn to_str(&mut self, v: Value) -> Result<Rc<str>, Value> {
        Ok(match v.kind() {
            Kind::Undefined => "undefined".into(),
            Kind::Null => "null".into(),
            Kind::Bool(b) => (if b { "true" } else { "false" }).into(),
            Kind::Number(n) => number_to_string(n).into(),
            Kind::String(r) => self.heap.string_rc(r),
            Kind::Object(_) => {
                let p = self.to_primitive(v, true)?;
                return self.to_str(p);
            }
        })
    }

    pub fn new_string(&mut self, s: &str) -> Value {
        Value::string(self.heap.alloc_string(s))
    }

    pub fn type_of(&self, v: Value) -> &'static str {
        match v.kind() {
            Kind::Undefined => "undefined",
            Kind::Null => "object",
            Kind::Bool(_) => "boolean",
            Kind::Number(_) => "number",
            Kind::String(_) => "string",
            Kind::Object(o) => {
                if self.heap.object(o).is_callable() {
                    "function"
                } else {
                    "object"
                }
            }
        }
    }

    pub fn strict_equals(&self, a: Value, b: Value) -> bool {
        match (a.kind(), b.kind()) {
            (Kind::Number(x), Kind::Number(y)) => x == y,
            (Kind::String(x), Kind::String(y)) => {
                x == y || self.heap.string(x) == self.heap.string(y)
            }
            (Kind::Object(x), Kind::Object(y)) => x == y,
            (Kind::Bool(x), Kind::Bool(y)) => x == y,
            (Kind::Undefined, Kind::Undefined) | (Kind::Null, Kind::Null) => true,
            _ => false,
        }
    }

    pub fn same_value(&self, a: Value, b: Value) -> bool {
        match (a.kind(), b.kind()) {
            (Kind::Number(x), Kind::Number(y)) => {
                (x.is_nan() && y.is_nan())
                    || (x == y && x.is_sign_negative() == y.is_sign_negative())
            }
            _ => self.strict_equals(a, b),
        }
    }

    pub fn loose_equals(&mut self, a: Value, b: Value) -> Result<bool, Value> {
        use Kind::*;
        Ok(match (a.kind(), b.kind()) {
            (Undefined | Null, Undefined | Null) => true,
            (Undefined | Null, _) | (_, Undefined | Null) => false,
            (Number(_), Number(_))
            | (String(_), String(_))
            | (Bool(_), Bool(_))
            | (Object(_), Object(_)) => self.strict_equals(a, b),
            (Number(x), String(_)) => x == self.to_number(b)?,
            (String(_), Number(y)) => self.to_number(a)? == y,
            (Bool(_), _) => {
                let n = Value::number(self.to_number(a)?);
                return self.loose_equals(n, b);
            }
            (_, Bool(_)) => {
                let n = Value::number(self.to_number(b)?);
                return self.loose_equals(a, n);
            }
            (Object(_), _) => {
                let p = self.to_primitive(a, false)?;
                return self.loose_equals(p, b);
            }
            (_, Object(_)) => {
                let p = self.to_primitive(b, false)?;
                return self.loose_equals(a, p);
            }
        })
    }

    // ---------------------------------------------------------- propriedades

    pub fn intern_value(&mut self, key: Value) -> Result<GcRef, Value> {
        let s = self.to_str(key)?;
        Ok(self.heap.intern(&s))
    }

    /// Procura a propriedade no objeto e na cadeia de protótipos.
    pub fn lookup(&self, mut o: GcRef, key: GcRef) -> Option<Value> {
        loop {
            let obj = self.heap.object(o);
            if let ObjectKind::Array(items) = &obj.kind {
                if key == self.names.length {
                    return Some(Value::number(items.len() as f64));
                }
                if let Some(i) = array_index(self.heap.string(key))
                    && i < items.len()
                {
                    return Some(items[i]);
                }
            }
            if let Some(v) = obj.get_own(key) {
                return Some(v);
            }
            o = obj.proto?;
        }
    }

    pub fn get_property(&mut self, target: Value, key: GcRef) -> Result<Value, Value> {
        match target.kind() {
            Kind::Object(o) => Ok(self.lookup(o, key).unwrap_or(Value::UNDEFINED)),
            Kind::String(s) => {
                if key == self.names.length {
                    let n = self.heap.string(s).encode_utf16().count();
                    return Ok(Value::number(n as f64));
                }
                if let Some(i) = array_index(self.heap.string(key)) {
                    let unit = self.heap.string(s).encode_utf16().nth(i);
                    return Ok(match unit {
                        Some(u) => {
                            let ch = String::from_utf16_lossy(&[u]);
                            self.new_string(&ch)
                        }
                        None => Value::UNDEFINED,
                    });
                }
                Ok(self
                    .lookup(self.protos.string, key)
                    .unwrap_or(Value::UNDEFINED))
            }
            Kind::Number(_) => Ok(self
                .lookup(self.protos.number, key)
                .unwrap_or(Value::UNDEFINED)),
            Kind::Bool(_) => Ok(self
                .lookup(self.protos.boolean, key)
                .unwrap_or(Value::UNDEFINED)),
            Kind::Undefined | Kind::Null => {
                let what = if target.is_undefined() {
                    "undefined"
                } else {
                    "null"
                };
                let k = self.heap.string(key).to_owned();
                Err(self.type_error(format!("Cannot read properties of {what} (reading '{k}')")))
            }
        }
    }

    pub fn set_property(&mut self, target: Value, key: GcRef, value: Value) -> Result<(), Value> {
        match target.kind() {
            Kind::Object(o) => {
                let is_length = key == self.names.length;
                let idx = array_index(self.heap.string(key));
                if let ObjectKind::Array(_) = self.heap.object(o).kind {
                    if is_length {
                        let n = self.to_number(value)?;
                        if n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
                            return Err(self.range_error("Invalid array length"));
                        }
                        if let ObjectKind::Array(items) = &mut self.heap.object_mut(o).kind {
                            items.resize(n as usize, Value::UNDEFINED);
                        }
                        return Ok(());
                    }
                    if let Some(i) = idx
                        && let ObjectKind::Array(items) = &mut self.heap.object_mut(o).kind
                        && i <= items.len() + 1_000_000
                    {
                        if i >= items.len() {
                            items.resize(i + 1, Value::UNDEFINED);
                        }
                        items[i] = value;
                        return Ok(());
                    }
                }
                self.heap.object_mut(o).set_own(key, value);
                Ok(())
            }
            Kind::Undefined | Kind::Null => {
                let k = self.heap.string(key).to_owned();
                Err(self.type_error(format!(
                    "Cannot set properties of {target:?} (setting '{k}')"
                )))
            }
            _ => Ok(()), // primitivos: ignorado (modo não estrito)
        }
    }

    fn get_keyed(&mut self, target: Value, key: Value) -> Result<Value, Value> {
        if let (Some(o), Some(n)) = (target.as_object(), key.as_number())
            && let ObjectKind::Array(items) = &self.heap.object(o).kind
            && n >= 0.0
            && n.fract() == 0.0
            && (n as usize) < items.len()
        {
            return Ok(items[n as usize]);
        }
        if target.is_nullish() {
            let k = self.to_str(key)?;
            return Err(self.type_error(format!(
                "Cannot read properties of {target:?} (reading '{k}')"
            )));
        }
        let k = self.intern_value(key)?;
        self.get_property(target, k)
    }

    fn set_keyed(&mut self, target: Value, key: Value, value: Value) -> Result<(), Value> {
        let k = self.intern_value(key)?;
        self.set_property(target, k, value)
    }

    fn has_property(&mut self, target: Value, key: Value) -> Result<bool, Value> {
        let Some(o) = target.as_object() else {
            return Err(
                self.type_error("Cannot use 'in' operator to search for a key in a primitive")
            );
        };
        let k = self.intern_value(key)?;
        Ok(self.lookup(o, k).is_some())
    }

    fn instance_of(&mut self, v: Value, ctor: Value) -> Result<bool, Value> {
        if !self.is_callable(ctor) {
            return Err(self.type_error("Right-hand side of 'instanceof' is not callable"));
        }
        let proto = self.get_property(ctor, self.names.prototype)?;
        let (Some(mut o), Some(proto)) = (v.as_object(), proto.as_object()) else {
            return Ok(false);
        };
        while let Some(p) = self.heap.object(o).proto {
            if p == proto {
                return Ok(true);
            }
            o = p;
        }
        Ok(false)
    }

    // -------------------------------------------------------------- funções

    pub fn make_function(&mut self, code: Rc<FunctionCode>) -> Value {
        let is_arrow = code.is_arrow;
        let name = self.heap.intern(&code.name);
        let params = code.params;
        let f = self
            .heap
            .alloc_object(Some(self.protos.function), ObjectKind::Function(code));
        let fv = Value::object(f);
        let (n_name, n_len, n_proto, n_ctor) = (
            self.names.name,
            self.names.length,
            self.names.prototype,
            self.names.constructor,
        );
        let obj = self.heap.object_mut(f);
        obj.set_own(n_name, Value::string(name));
        obj.set_own(n_len, Value::number(params as f64));
        if !is_arrow {
            let proto = self
                .heap
                .alloc_object(Some(self.protos.object), ObjectKind::Ordinary);
            self.heap.object_mut(proto).set_own(n_ctor, fv);
            self.heap
                .object_mut(f)
                .set_own(n_proto, Value::object(proto));
        }
        fv
    }

    /// Chama uma função a partir do Rust e espera o resultado.
    pub fn call_function(&mut self, f: Value, this: Value, args: &[Value]) -> Result<Value, Value> {
        let Some(o) = f.as_object() else {
            return Err(self.type_error("value is not a function"));
        };
        match &self.heap.object(o).kind {
            ObjectKind::Native { func, .. } => {
                let func = *func;
                self.native_depth += 1;
                let r = func(self, this, args);
                self.native_depth -= 1;
                r
            }
            ObjectKind::Function(code) => {
                let code = code.clone();
                let depth = self.frames.len();
                self.push_frame(code, this, args, None, None)?;
                self.native_depth += 1;
                let r = self.run(depth);
                self.native_depth -= 1;
                r
            }
            _ => Err(self.type_error("value is not a function")),
        }
    }

    fn push_frame(
        &mut self,
        code: Rc<FunctionCode>,
        this: Value,
        args: &[Value],
        ret_dst: Option<usize>,
        construct: Option<GcRef>,
    ) -> Result<(), Value> {
        let base = self.enter(&code, this, ret_dst, construct)?;
        for (i, a) in args.iter().take(code.params as usize).enumerate() {
            self.regs[base + i] = *a;
        }
        Ok(())
    }

    /// Igual a `push_frame`, mas os argumentos já estão nos registradores do chamador
    /// (copiados sem alocar).
    fn push_frame_from_regs(
        &mut self,
        code: Rc<FunctionCode>,
        this: Value,
        args_abs: usize,
        argc: usize,
        ret_dst: Option<usize>,
        construct: Option<GcRef>,
    ) -> Result<(), Value> {
        let n = argc.min(code.params as usize);
        let base = self.enter(&code, this, ret_dst, construct)?;
        self.regs.copy_within(args_abs..args_abs + n, base);
        Ok(())
    }

    fn enter(
        &mut self,
        code: &Rc<FunctionCode>,
        this: Value,
        ret_dst: Option<usize>,
        construct: Option<GcRef>,
    ) -> Result<usize, Value> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(self.range_error("Maximum call stack size exceeded"));
        }
        let base = self.regs.len();
        self.regs
            .resize(base + code.registers as usize, Value::UNDEFINED);
        let this = if !code.is_arrow && this.is_nullish() {
            Value::object(self.global)
        } else {
            this
        };
        self.frames.push(Frame {
            code: code.clone(),
            pc: 0,
            base,
            this,
            ret_dst,
            construct,
            handlers: Vec::new(),
        });
        Ok(base)
    }

    fn not_callable(&mut self, v: Value) -> Value {
        let t = self.type_of(v);
        self.type_error(format!("{t} is not a function"))
    }

    // ------------------------------------------------------------ execução

    fn run(&mut self, entry_depth: usize) -> Result<Value, Value> {
        loop {
            match self.run_inner(entry_depth) {
                Ok(v) => return Ok(v),
                Err(exc) => self.unwind(exc, entry_depth)?,
            }
        }
    }

    fn unwind(&mut self, exc: Value, entry_depth: usize) -> Result<(), Value> {
        while self.frames.len() > entry_depth {
            let frame = self.frames.last_mut().expect("frame");
            if let Some((target, reg)) = frame.handlers.pop() {
                frame.pc = target as usize;
                let at = frame.base + reg as usize;
                self.regs[at] = exc;
                return Ok(());
            }
            let f = self.frames.pop().expect("frame");
            self.regs.truncate(f.base);
        }
        Err(exc)
    }

    fn binary(&mut self, op: BinOp, a: Value, b: Value) -> Result<Value, Value> {
        use BinOp::*;
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            // caminho rápido: dois números
            let r = match op {
                Add => Some(x + y),
                Sub => Some(x - y),
                Mul => Some(x * y),
                Div => Some(x / y),
                Rem => Some(x % y),
                Lt => return Ok(Value::boolean(x < y)),
                Le => return Ok(Value::boolean(x <= y)),
                Gt => return Ok(Value::boolean(x > y)),
                Ge => return Ok(Value::boolean(x >= y)),
                Eq | StrictEq => return Ok(Value::boolean(x == y)),
                Ne | StrictNe => return Ok(Value::boolean(x != y)),
                _ => None,
            };
            if let Some(r) = r {
                return Ok(Value::number(r));
            }
        }
        Ok(match op {
            Add => {
                let pa = self.to_primitive(a, false)?;
                let pb = self.to_primitive(b, false)?;
                if pa.as_string().is_some() || pb.as_string().is_some() {
                    let sa = self.to_str(pa)?;
                    let sb = self.to_str(pb)?;
                    let mut s = String::with_capacity(sa.len() + sb.len());
                    s.push_str(&sa);
                    s.push_str(&sb);
                    self.new_string(&s)
                } else {
                    Value::number(self.to_number(pa)? + self.to_number(pb)?)
                }
            }
            Sub | Mul | Div | Rem | Exp => {
                let x = self.to_number(a)?;
                let y = self.to_number(b)?;
                Value::number(match op {
                    Sub => x - y,
                    Mul => x * y,
                    Div => x / y,
                    Rem => x % y,
                    _ => js_pow(x, y),
                })
            }
            Lt | Le | Gt | Ge => {
                let pa = self.to_primitive(a, false)?;
                let pb = self.to_primitive(b, false)?;
                if let (Some(sa), Some(sb)) = (pa.as_string(), pb.as_string()) {
                    let (sa, sb) = (self.heap.string(sa), self.heap.string(sb));
                    let ord = sa.encode_utf16().cmp(sb.encode_utf16());
                    Value::boolean(match op {
                        Lt => ord.is_lt(),
                        Le => ord.is_le(),
                        Gt => ord.is_gt(),
                        _ => ord.is_ge(),
                    })
                } else {
                    let x = self.to_number(pa)?;
                    let y = self.to_number(pb)?;
                    Value::boolean(match op {
                        Lt => x < y,
                        Le => x <= y,
                        Gt => x > y,
                        _ => x >= y,
                    })
                }
            }
            Eq => Value::boolean(self.loose_equals(a, b)?),
            Ne => Value::boolean(!self.loose_equals(a, b)?),
            StrictEq => Value::boolean(self.strict_equals(a, b)),
            StrictNe => Value::boolean(!self.strict_equals(a, b)),
            BitAnd | BitOr | BitXor | Shl | Shr => {
                let x = to_int32(self.to_number(a)?);
                let y = to_int32(self.to_number(b)?);
                Value::number(match op {
                    BitAnd => x & y,
                    BitOr => x | y,
                    BitXor => x ^ y,
                    Shl => x.wrapping_shl(y as u32 & 31),
                    _ => x >> (y as u32 & 31),
                } as f64)
            }
            UShr => {
                let x = to_int32(self.to_number(a)?) as u32;
                let y = to_int32(self.to_number(b)?) as u32;
                Value::number((x >> (y & 31)) as f64)
            }
            In => Value::boolean(self.has_property(b, a)?),
            InstanceOf => Value::boolean(self.instance_of(a, b)?),
        })
    }

    fn unary(&mut self, op: UnOp, v: Value) -> Result<Value, Value> {
        Ok(match op {
            UnOp::Neg => Value::number(-self.to_number(v)?),
            UnOp::Plus => Value::number(self.to_number(v)?),
            UnOp::Not => Value::boolean(!self.to_boolean(v)),
            UnOp::BitNot => Value::number(!to_int32(self.to_number(v)?) as f64),
            UnOp::TypeOf => {
                let t = self.type_of(v);
                Value::string(self.heap.intern(t))
            }
            UnOp::Void => Value::UNDEFINED,
        })
    }

    fn run_inner(&mut self, entry_depth: usize) -> Result<Value, Value> {
        loop {
            let frame = self.frames.last_mut().expect("sem frame");
            let op = frame.code.code[frame.pc];
            frame.pc += 1;
            let base = frame.base;
            self.steps += 1;
            if let Some(limit) = self.step_limit
                && self.steps > limit
            {
                return Err(self.range_error("limite de instruções atingido"));
            }
            let r = |reg: Reg| base + reg as usize;
            match op {
                Op::LoadConst { dst, idx } => {
                    let v = self.frames.last().expect("frame").code.constants[idx as usize];
                    self.regs[r(dst)] = v;
                }
                Op::LoadUndefined { dst } => self.regs[r(dst)] = Value::UNDEFINED,
                Op::LoadNull { dst } => self.regs[r(dst)] = Value::NULL,
                Op::LoadBool { dst, value } => self.regs[r(dst)] = Value::boolean(value),
                Op::LoadThis { dst } => {
                    self.regs[r(dst)] = self.frames.last().expect("frame").this;
                }
                Op::Move { dst, src } => self.regs[r(dst)] = self.regs[r(src)],
                Op::Binary { op, dst, lhs, rhs } => {
                    let v = self.binary(op, self.regs[r(lhs)], self.regs[r(rhs)])?;
                    self.regs[r(dst)] = v;
                }
                Op::Unary { op, dst, src } => {
                    let v = self.unary(op, self.regs[r(src)])?;
                    self.regs[r(dst)] = v;
                }
                Op::Jump { target } => self.jump(target),
                Op::JumpIfFalse { cond, target } => {
                    if !self.to_boolean(self.regs[r(cond)]) {
                        self.jump(target);
                    }
                }
                Op::JumpIfTrue { cond, target } => {
                    if self.to_boolean(self.regs[r(cond)]) {
                        self.jump(target);
                    }
                }
                Op::JumpIfNotNullish { cond, target } => {
                    if !self.regs[r(cond)].is_nullish() {
                        self.jump(target);
                    }
                }
                Op::GetGlobal { dst, name } => {
                    let key = self.const_name(name);
                    match self.lookup(self.global, key) {
                        Some(v) => self.regs[r(dst)] = v,
                        None => {
                            let n = self.heap.string(key).to_owned();
                            return Err(self.reference_error(format!("{n} is not defined")));
                        }
                    }
                }
                Op::TypeOfGlobal { dst, name } => {
                    let key = self.const_name(name);
                    let v = self.lookup(self.global, key).unwrap_or(Value::UNDEFINED);
                    let t = self.type_of(v);
                    self.regs[r(dst)] = Value::string(self.heap.intern(t));
                }
                Op::SetGlobal { name, src } => {
                    let key = self.const_name(name);
                    let v = self.regs[r(src)];
                    self.heap.object_mut(self.global).set_own(key, v);
                }
                Op::DeclareGlobal { name } => {
                    let key = self.const_name(name);
                    let g = self.heap.object_mut(self.global);
                    if g.get_own(key).is_none() {
                        g.set_own(key, Value::UNDEFINED);
                    }
                }
                Op::GetNamed { dst, obj, name } => {
                    let key = self.const_name(name);
                    let v = self.get_property(self.regs[r(obj)], key)?;
                    self.regs[r(dst)] = v;
                }
                Op::SetNamed { obj, name, src } => {
                    let key = self.const_name(name);
                    self.set_property(self.regs[r(obj)], key, self.regs[r(src)])?;
                }
                Op::GetKeyed { dst, obj, key } => {
                    let v = self.get_keyed(self.regs[r(obj)], self.regs[r(key)])?;
                    self.regs[r(dst)] = v;
                }
                Op::SetKeyed { obj, key, src } => {
                    self.set_keyed(self.regs[r(obj)], self.regs[r(key)], self.regs[r(src)])?;
                }
                Op::NewObject { dst } => {
                    self.maybe_gc();
                    let o = self
                        .heap
                        .alloc_object(Some(self.protos.object), ObjectKind::Ordinary);
                    self.regs[r(dst)] = Value::object(o);
                }
                Op::NewArray { dst } => {
                    self.maybe_gc();
                    let o = self
                        .heap
                        .alloc_object(Some(self.protos.array), ObjectKind::Array(Vec::new()));
                    self.regs[r(dst)] = Value::object(o);
                }
                Op::ArrayPush { array, src } => {
                    let v = self.regs[r(src)];
                    if let Some(o) = self.regs[r(array)].as_object()
                        && let ObjectKind::Array(items) = &mut self.heap.object_mut(o).kind
                    {
                        items.push(v);
                    }
                }
                Op::Closure { dst, func } => {
                    self.maybe_gc();
                    let code =
                        self.frames.last().expect("frame").code.functions[func as usize].clone();
                    let f = self.make_function(code);
                    self.regs[r(dst)] = f;
                }
                Op::Call {
                    dst,
                    callee,
                    this,
                    args,
                    argc,
                } => {
                    self.maybe_gc();
                    let f = self.regs[r(callee)];
                    let this_v = self.regs[r(this)];
                    let Some(o) = f.as_object() else {
                        return Err(self.not_callable(f));
                    };
                    match &self.heap.object(o).kind {
                        ObjectKind::Function(code) => {
                            let code = code.clone();
                            self.push_frame_from_regs(
                                code,
                                this_v,
                                r(args),
                                argc as usize,
                                Some(r(dst)),
                                None,
                            )?;
                        }
                        ObjectKind::Native { func, .. } => {
                            let func = *func;
                            let argv = self.regs[r(args)..r(args) + argc as usize].to_vec();
                            self.native_depth += 1;
                            let res = func(self, this_v, &argv);
                            self.native_depth -= 1;
                            self.regs[r(dst)] = res?;
                        }
                        _ => return Err(self.not_callable(f)),
                    }
                }
                Op::New {
                    dst,
                    callee,
                    args,
                    argc,
                } => {
                    self.maybe_gc();
                    let f = self.regs[r(callee)];
                    let Some(o) = f.as_object() else {
                        return Err(self.type_error("value is not a constructor"));
                    };
                    let proto_v = self.get_property(f, self.names.prototype)?;
                    let proto = proto_v.as_object().unwrap_or(self.protos.object);
                    match &self.heap.object(o).kind {
                        ObjectKind::Function(code) if !code.is_arrow => {
                            let code = code.clone();
                            let obj = self.heap.alloc_object(Some(proto), ObjectKind::Ordinary);
                            self.push_frame_from_regs(
                                code,
                                Value::object(obj),
                                r(args),
                                argc as usize,
                                Some(r(dst)),
                                Some(obj),
                            )?;
                        }
                        ObjectKind::Native {
                            func,
                            constructor: true,
                        } => {
                            let func = *func;
                            let obj = self.heap.alloc_object(Some(proto), ObjectKind::Ordinary);
                            let argv = self.regs[r(args)..r(args) + argc as usize].to_vec();
                            self.native_depth += 1;
                            let res = func(self, Value::object(obj), &argv);
                            self.native_depth -= 1;
                            let v = res?;
                            self.regs[r(dst)] = if v.as_object().is_some() {
                                v
                            } else {
                                Value::object(obj)
                            };
                        }
                        _ => return Err(self.type_error("value is not a constructor")),
                    }
                }
                Op::Return { src } => {
                    let v = self.regs[r(src)];
                    let frame = self.frames.pop().expect("frame");
                    self.regs.truncate(frame.base);
                    let v = match frame.construct {
                        Some(obj) if v.as_object().is_none() => Value::object(obj),
                        _ => v,
                    };
                    if self.frames.len() == entry_depth {
                        return Ok(v);
                    }
                    if let Some(dst) = frame.ret_dst {
                        self.regs[dst] = v;
                    }
                }
                Op::Throw { src } => return Err(self.regs[r(src)]),
                Op::EnterTry { catch_target, exc } => {
                    self.frames
                        .last_mut()
                        .expect("frame")
                        .handlers
                        .push((catch_target, exc));
                }
                Op::ExitTry => {
                    self.frames.last_mut().expect("frame").handlers.pop();
                }
            }
        }
    }

    #[inline]
    fn jump(&mut self, target: u32) {
        self.frames.last_mut().expect("frame").pc = target as usize;
    }

    #[inline]
    fn const_name(&self, idx: u32) -> GcRef {
        self.frames.last().expect("frame").code.constants[idx as usize]
            .as_string()
            .expect("nome constante não é string")
    }
}

// ------------------------------------------------------------------ helpers

pub fn number_to_string(n: f64) -> String {
    let mut buf = ryu_js::Buffer::new();
    buf.format(n).to_owned()
}

pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = t.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix)
                .map(|v| v as f64)
                .unwrap_or(f64::NAN);
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.chars()
        .any(|c| !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')))
    {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

pub fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let m = n.trunc().rem_euclid(4_294_967_296.0);
    m as u32 as i32
}

fn js_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() || (x.abs() == 1.0 && y.is_infinite()) {
        return f64::NAN;
    }
    x.powf(y)
}

/// Índice de array canônico ("0", "15"; não "01" nem "-1").
pub fn array_index(s: &str) -> Option<usize> {
    if s.is_empty() || s.len() > 10 || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = s.parse().ok()?;
    (n < u32::MAX as u64).then_some(n as usize)
}
