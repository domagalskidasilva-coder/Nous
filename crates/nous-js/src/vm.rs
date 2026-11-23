//! Interpretador do bytecode.
//!
//! Chamadas entre funções JavaScript não usam a pilha do Rust: cada chamada empilha um
//! `Frame` e o mesmo laço continua. Só quando o Rust chama JavaScript (funções nativas,
//! conversões) é que um laço aninhado roda; nesses trechos o GC fica bloqueado, porque
//! pode haver valores vivos guardados em variáveis do Rust.

use std::collections::HashSet;
use std::rc::Rc;

use crate::bytecode::{BinOp, FunctionCode, Op, Reg, UnOp};
use crate::heap::{
    FunctionObj, GcRef, Heap, IterKind, MapIterMode, ObjectKind, PROP_ENUMERABLE, PROP_METHOD,
    UpvalueState,
};
use crate::host::TimerEntry;
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
    pub size: GcRef,
    pub done: GcRef,
    pub value: GcRef,
    pub next: GcRef,
    pub iterator: GcRef,
}

pub(crate) struct Protos {
    pub object: GcRef,
    pub function: GcRef,
    pub array: GcRef,
    pub string: GcRef,
    pub number: GcRef,
    pub boolean: GcRef,
    pub map: GcRef,
    pub set: GcRef,
    pub iterator: GcRef,
    pub error: GcRef,
    pub type_error: GcRef,
    pub reference_error: GcRef,
    pub range_error: GcRef,
    pub syntax_error: GcRef,
}

pub(crate) struct Frame {
    pub code: Rc<FunctionCode>,
    pub callee: Option<GcRef>,
    pub upvalues: Rc<[GcRef]>,
    pub home_object: Option<GcRef>,
    pub super_ctor: Option<GcRef>,
    pub super_called: bool,
    pub pc: usize,
    pub base: usize,
    pub this: Value,
    /// Registrador absoluto do chamador que recebe o retorno (`None` = chamada vinda do Rust).
    pub ret_dst: Option<usize>,
    /// Objeto criado por `new`, devolvido se a função não retornar outro objeto.
    pub construct: Option<GcRef>,
    pub handlers: Vec<(u32, Reg)>,
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
    pub(crate) regs: Vec<Value>,
    pub(crate) frames: Vec<Frame>,
    pub(crate) open_upvalues: Vec<(usize, GcRef)>,
    pub(crate) timers: Vec<TimerEntry>,
    pub(crate) next_timer_id: u64,
    pub(crate) names: Names,
    pub(crate) protos: Protos,
    native_depth: u32,
    /// Limite de instruções (para o runner do test262 não travar em laços infinitos).
    pub step_limit: Option<u64>,
    pub(crate) steps: u64,
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
            size: heap.intern("size"),
            done: heap.intern("done"),
            value: heap.intern("value"),
            next: heap.intern("next"),
            iterator: heap.intern("@@iterator"),
        };
        let object = heap.alloc_object(None, ObjectKind::Ordinary);
        let mut proto = |kind| heap.alloc_object(Some(object), kind);
        let function = proto(ObjectKind::Ordinary);
        let array = proto(ObjectKind::Array(Vec::new()));
        let string = proto(ObjectKind::Ordinary);
        let number = proto(ObjectKind::Ordinary);
        let boolean = proto(ObjectKind::Ordinary);
        let map = proto(ObjectKind::Ordinary);
        let set = proto(ObjectKind::Ordinary);
        let iterator = proto(ObjectKind::Ordinary);
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
            open_upvalues: Vec::new(),
            timers: Vec::new(),
            next_timer_id: 1,
            names,
            protos: Protos {
                object,
                function,
                array,
                string,
                number,
                boolean,
                map,
                set,
                iterator,
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
        crate::host::install_timers(&mut vm);
        vm
    }

    // ------------------------------------------------------------ API pública

    /// Compila e roda um script. Devolve o valor da última expressão.
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        let code = crate::compiler::compile_script(&mut self.heap, source)?;
        let depth = self.frames.len();
        let this = Value::object(self.global);
        if depth == 0 {
            self.steps = 0;
        }
        if let Err(e) = self.push_frame(
            Rc::new(code),
            None,
            Rc::from([]),
            None,
            None,
            this,
            &[],
            None,
            None,
        ) {
            return Err(self.value_to_js_error(e));
        }
        match self.run(depth) {
            Ok(v) => Ok(v),
            Err(e) => Err(self.value_to_js_error(e)),
        }
    }

    pub(crate) fn value_to_js_error(&mut self, e: Value) -> JsError {
        JsError::Thrown(self.display(e))
    }

    pub(crate) fn js_error_to_value(&mut self, e: JsError) -> Value {
        match e {
            JsError::Syntax(m) => self.make_error(self.protos.syntax_error, &m),
            JsError::Unsupported(m) => self.make_error(self.protos.error, &m),
            JsError::Thrown(m) => {
                if let Some(rest) = m.strip_prefix("TypeError: ") {
                    self.type_error(rest)
                } else if let Some(rest) = m.strip_prefix("RangeError: ") {
                    self.range_error(rest)
                } else if let Some(rest) = m.strip_prefix("ReferenceError: ") {
                    self.reference_error(rest)
                } else if let Some(rest) = m.strip_prefix("SyntaxError: ") {
                    self.make_error(self.protos.syntax_error, rest)
                } else {
                    self.make_error(self.protos.error, &m)
                }
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
        let n = &self.names;
        for r in [
            n.length,
            n.prototype,
            n.constructor,
            n.message,
            n.name,
            n.to_string,
            n.value_of,
            n.size,
            n.done,
            n.value,
            n.next,
            n.iterator,
        ] {
            roots.push(Value::string(r));
        }
        let p = &self.protos;
        for r in [
            p.object,
            p.function,
            p.array,
            p.string,
            p.number,
            p.boolean,
            p.map,
            p.set,
            p.iterator,
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
            if let Some(callee) = f.callee {
                roots.push(Value::object(callee));
            }
            if let Some(h) = f.home_object {
                roots.push(Value::object(h));
            }
            if let Some(sc) = f.super_ctor {
                roots.push(Value::object(sc));
            }
            for &uv in f.upvalues.iter() {
                roots.push(Value::object(uv));
            }
            push_code_constants(&f.code, &mut roots);
        }
        for t in &self.timers {
            roots.push(t.callback);
            roots.extend_from_slice(&t.args);
        }
        self.heap.collect(roots);
        self.open_upvalues.retain(|&(_, uv)| self.heap.is_live(uv));
    }

    pub(crate) fn maybe_gc(&mut self) {
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

    pub fn syntax_error(&mut self, msg: impl AsRef<str>) -> Value {
        self.make_error(self.protos.syntax_error, msg.as_ref())
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

    pub fn same_value_zero(&self, a: Value, b: Value) -> bool {
        match (a.kind(), b.kind()) {
            (Kind::Number(x), Kind::Number(y)) => (x.is_nan() && y.is_nan()) || x == y,
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
            match &obj.kind {
                ObjectKind::Array(items) => {
                    if key == self.names.length {
                        return Some(Value::number(items.len() as f64));
                    }
                    if let Some(i) = array_index(self.heap.string(key))
                        && i < items.len()
                    {
                        return Some(items[i]);
                    }
                }
                ObjectKind::Map(m) if key == self.names.size => {
                    return Some(Value::number(m.len as f64));
                }
                ObjectKind::Set(s) if key == self.names.size => {
                    return Some(Value::number(s.len as f64));
                }
                _ => {}
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
                if self.heap.object(o).frozen {
                    return Ok(());
                }
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

    pub(crate) fn delete_property(&mut self, target: Value, key: GcRef) -> Result<bool, Value> {
        match target.kind() {
            Kind::Object(o) => {
                if self.heap.object(o).frozen {
                    return Ok(false);
                }
                let idx = array_index(self.heap.string(key));
                if let Some(i) = idx
                    && let ObjectKind::Array(items) = &mut self.heap.object_mut(o).kind
                {
                    if i < items.len() {
                        items[i] = Value::UNDEFINED;
                    }
                    return Ok(true);
                }
                Ok(self.heap.object_mut(o).delete_own(key))
            }
            Kind::Undefined | Kind::Null => {
                let k = self.heap.string(key).to_owned();
                Err(self.type_error(format!(
                    "Cannot convert undefined or null to object (deleting '{k}')"
                )))
            }
            _ => Ok(true),
        }
    }

    pub(crate) fn get_keyed(&mut self, target: Value, key: Value) -> Result<Value, Value> {
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

    pub(crate) fn set_keyed(
        &mut self,
        target: Value,
        key: Value,
        value: Value,
    ) -> Result<(), Value> {
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

    // -------------------------------------------------------------- funções e closures

    pub(crate) fn alloc_native_fn(
        &mut self,
        name: &str,
        func: NativeFn,
        length: u32,
        constructor: bool,
    ) -> GcRef {
        let f = self.heap.alloc_object(
            Some(self.protos.function),
            ObjectKind::Native { func, constructor },
        );
        let n = self.heap.intern(name);
        let (k_name, k_len) = (self.names.name, self.names.length);
        let obj = self.heap.object_mut(f);
        obj.define_own(k_name, Value::string(n), PROP_METHOD);
        obj.define_own(k_len, Value::number(length as f64), PROP_METHOD);
        f
    }

    pub fn make_function(&mut self, code: Rc<FunctionCode>) -> Value {
        self.make_closure(code, Rc::from([]), Value::UNDEFINED, None, None)
    }

    pub(crate) fn make_closure(
        &mut self,
        code: Rc<FunctionCode>,
        upvalues: Rc<[GcRef]>,
        bound_this: Value,
        home_object: Option<GcRef>,
        super_ctor: Option<GcRef>,
    ) -> Value {
        let is_arrow = code.is_arrow;
        let name = self.heap.intern(&code.name);
        let length = code.length;
        let f = self.heap.alloc_object(
            Some(self.protos.function),
            ObjectKind::Function(FunctionObj {
                code,
                upvalues,
                bound_this,
                home_object,
                super_ctor,
            }),
        );
        let fv = Value::object(f);
        let (n_name, n_len, n_proto, n_ctor) = (
            self.names.name,
            self.names.length,
            self.names.prototype,
            self.names.constructor,
        );
        let obj = self.heap.object_mut(f);
        obj.define_own(n_name, Value::string(name), PROP_METHOD);
        obj.define_own(n_len, Value::number(length as f64), PROP_METHOD);
        if !is_arrow {
            let proto = self
                .heap
                .alloc_object(Some(self.protos.object), ObjectKind::Ordinary);
            self.heap
                .object_mut(proto)
                .define_own(n_ctor, fv, PROP_METHOD);
            self.heap
                .object_mut(f)
                .define_own(n_proto, Value::object(proto), PROP_METHOD);
        }
        fv
    }

    fn capture_upvalue(&mut self, abs_reg: usize) -> GcRef {
        if let Some(&(_, uv)) = self.open_upvalues.iter().find(|&&(r, _)| r == abs_reg) {
            return uv;
        }
        let uv = self.heap.alloc_upvalue(abs_reg);
        self.open_upvalues.push((abs_reg, uv));
        uv
    }

    fn close_upvalues_from(&mut self, min_abs: usize) {
        if self.open_upvalues.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let (abs_reg, uv) = self.open_upvalues[i];
            if abs_reg >= min_abs {
                let val = self.regs[abs_reg];
                self.heap.close_upvalue(uv, val);
                self.open_upvalues.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Chama uma função a partir do Rust e espera o resultado.
    pub fn call_function(&mut self, f: Value, this: Value, args: &[Value]) -> Result<Value, Value> {
        let Some(o) = f.as_object() else {
            return Err(self.not_callable(f));
        };
        match &self.heap.object(o).kind {
            ObjectKind::Native { func, .. } => {
                let func = *func;
                self.native_depth += 1;
                let r = func(self, this, args);
                self.native_depth -= 1;
                r
            }
            ObjectKind::HostFunction { func, .. } => {
                let func = func.clone();
                self.native_depth += 1;
                let r = func(self, this, args).map_err(|e| self.js_error_to_value(e));
                self.native_depth -= 1;
                r
            }
            ObjectKind::Function(fn_obj) => {
                if fn_obj.code.is_class_ctor {
                    return Err(self.type_error("Class constructor cannot be invoked without 'new'"));
                }
                let code = fn_obj.code.clone();
                let uvs = fn_obj.upvalues.clone();
                let home = fn_obj.home_object;
                let sc = fn_obj.super_ctor;
                let eff_this = if code.is_arrow {
                    fn_obj.bound_this
                } else {
                    this
                };
                let depth = self.frames.len();
                self.push_frame(code, Some(o), uvs, home, sc, eff_this, args, None, None)?;
                self.native_depth += 1;
                let r = self.run(depth);
                self.native_depth -= 1;
                r
            }
            _ => Err(self.not_callable(f)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_frame(
        &mut self,
        code: Rc<FunctionCode>,
        callee: Option<GcRef>,
        upvalues: Rc<[GcRef]>,
        home_object: Option<GcRef>,
        super_ctor: Option<GcRef>,
        this: Value,
        args: &[Value],
        ret_dst: Option<usize>,
        construct: Option<GcRef>,
    ) -> Result<(), Value> {
        let base = self.enter(
            &code,
            callee,
            upvalues,
            home_object,
            super_ctor,
            this,
            ret_dst,
            construct,
        )?;
        let n = args.len().min(code.params as usize);
        for (i, a) in args.iter().take(n).enumerate() {
            self.regs[base + i] = *a;
        }
        if code.has_rest {
            let rest = if args.len() > code.params as usize {
                args[code.params as usize..].to_vec()
            } else {
                Vec::new()
            };
            let arr = self
                .heap
                .alloc_object(Some(self.protos.array), ObjectKind::Array(rest));
            self.regs[base + code.params as usize] = Value::object(arr);
        }
        Ok(())
    }

    /// Igual a `push_frame`, mas os argumentos já estão nos registradores do chamador.
    #[allow(clippy::too_many_arguments)]
    fn push_frame_from_regs(
        &mut self,
        code: Rc<FunctionCode>,
        callee: Option<GcRef>,
        upvalues: Rc<[GcRef]>,
        home_object: Option<GcRef>,
        super_ctor: Option<GcRef>,
        this: Value,
        args_abs: usize,
        argc: usize,
        ret_dst: Option<usize>,
        construct: Option<GcRef>,
    ) -> Result<(), Value> {
        let n = argc.min(code.params as usize);
        let base = self.enter(
            &code,
            callee,
            upvalues,
            home_object,
            super_ctor,
            this,
            ret_dst,
            construct,
        )?;
        self.regs.copy_within(args_abs..args_abs + n, base);
        if code.has_rest {
            let rest = if argc > code.params as usize {
                self.regs[args_abs + code.params as usize..args_abs + argc].to_vec()
            } else {
                Vec::new()
            };
            let arr = self
                .heap
                .alloc_object(Some(self.protos.array), ObjectKind::Array(rest));
            self.regs[base + code.params as usize] = Value::object(arr);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn enter(
        &mut self,
        code: &Rc<FunctionCode>,
        callee: Option<GcRef>,
        upvalues: Rc<[GcRef]>,
        home_object: Option<GcRef>,
        super_ctor: Option<GcRef>,
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
            callee,
            upvalues,
            home_object,
            super_ctor,
            super_called: false,
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

    // ------------------------------------------------------------ iteradores & spread

    pub(crate) fn make_iterator(&mut self, src: Value) -> Result<GcRef, Value> {
        let kind = match src.kind() {
            Kind::String(s) => IterKind::String { s, byte_pos: 0 },
            Kind::Object(o) => match &self.heap.object(o).kind {
                ObjectKind::Array(_) => IterKind::Array { arr: o, idx: 0 },
                ObjectKind::Map(_) => IterKind::Map {
                    map: o,
                    idx: 0,
                    mode: MapIterMode::Entries,
                },
                ObjectKind::Set(_) => IterKind::Set {
                    set: o,
                    idx: 0,
                    entries_mode: false,
                },
                ObjectKind::Iterator(_) => return Ok(o),
                _ => {
                    let iter_fn = self.lookup(o, self.names.iterator).unwrap_or(Value::UNDEFINED);
                    if self.is_callable(iter_fn) {
                        let iter_obj = self.call_function(iter_fn, src, &[])?;
                        let next_fn = self.get_property(iter_obj, self.names.next)?;
                        if !self.is_callable(next_fn) {
                            return Err(self.type_error("Iterator must have a callable next()"));
                        }
                        IterKind::Custom {
                            obj: iter_obj,
                            next_fn,
                        }
                    } else {
                        return Err(self.type_error("value is not iterable"));
                    }
                }
            },
            _ => return Err(self.type_error("value is not iterable")),
        };
        Ok(self
            .heap
            .alloc_object(Some(self.protos.iterator), ObjectKind::Iterator(kind)))
    }

    pub(crate) fn iter_next(&mut self, iter_ref: GcRef) -> Result<Option<Value>, Value> {
        let step = {
            let ObjectKind::Iterator(it) = &mut self.heap.object_mut(iter_ref).kind else {
                return Err(self.type_error("expected iterator"));
            };
            match it {
                IterKind::Empty => Ok(None),
                IterKind::Array { arr, idx } => {
                    let arr = *arr;
                    let i = *idx;
                    *idx += 1;
                    Err((0u8, arr, i))
                }
                IterKind::String { s, byte_pos } => {
                    let s = *s;
                    let pos = *byte_pos;
                    Err((1u8, s, pos))
                }
                IterKind::Keys { keys, idx } => {
                    if *idx < keys.len() {
                        let k = keys[*idx];
                        *idx += 1;
                        Ok(Some(Value::string(k)))
                    } else {
                        Ok(None)
                    }
                }
                IterKind::Values { items, idx } => {
                    if *idx < items.len() {
                        let v = items[*idx];
                        *idx += 1;
                        Ok(Some(v))
                    } else {
                        Ok(None)
                    }
                }
                IterKind::Map { map, idx, mode } => {
                    let map = *map;
                    let i = *idx;
                    let mode_u = match *mode {
                        MapIterMode::Entries => 2u8,
                        MapIterMode::Keys => 3u8,
                        MapIterMode::Values => 4u8,
                    };
                    Err((mode_u, map, i))
                }
                IterKind::Set {
                    set,
                    idx,
                    entries_mode,
                } => {
                    let set = *set;
                    let i = *idx;
                    let m = if *entries_mode { 5u8 } else { 6u8 };
                    Err((m, set, i))
                }
                IterKind::Custom { obj, next_fn } => {
                    let obj = *obj;
                    let next_fn = *next_fn;
                    let res = self.call_function(next_fn, obj, &[])?;
                    let done = self.get_property(res, self.names.done)?;
                    if self.to_boolean(done) {
                        return Ok(None);
                    }
                    let val = self.get_property(res, self.names.value)?;
                    return Ok(Some(val));
                }
            }
        };

        match step {
            Ok(v) => Ok(v),
            Err((0, arr, i)) => {
                let item = match &self.heap.object(arr).kind {
                    ObjectKind::Array(items) if i < items.len() => Some(items[i]),
                    _ => None,
                };
                Ok(item)
            }
            Err((1, s_ref, pos)) => {
                let s = self.heap.string(s_ref);
                if pos >= s.len() {
                    return Ok(None);
                }
                let ch = s[pos..].chars().next().expect("char");
                let next_pos = pos + ch.len_utf8();
                let mut buf = [0u8; 4];
                let ch_str = ch.encode_utf8(&mut buf).to_owned();
                if let ObjectKind::Iterator(IterKind::String { byte_pos, .. }) =
                    &mut self.heap.object_mut(iter_ref).kind
                {
                    *byte_pos = next_pos;
                }
                Ok(Some(self.new_string(&ch_str)))
            }
            Err((mode_u, map_ref, start_idx)) => {
                if mode_u <= 4 {
                    let found = match &self.heap.object(map_ref).kind {
                        ObjectKind::Map(m) => m.entries[start_idx..]
                            .iter()
                            .enumerate()
                            .find_map(|(off, e)| e.map(|pair| (start_idx + off + 1, pair))),
                        _ => None,
                    };
                    let Some((next_idx, (k, v))) = found else {
                        return Ok(None);
                    };
                    if let ObjectKind::Iterator(IterKind::Map { idx, .. }) =
                        &mut self.heap.object_mut(iter_ref).kind
                    {
                        *idx = next_idx;
                    }
                    Ok(Some(match mode_u {
                        2 => {
                            let arr = self
                                .heap
                                .alloc_object(Some(self.protos.array), ObjectKind::Array(vec![k, v]));
                            Value::object(arr)
                        }
                        3 => k,
                        _ => v,
                    }))
                } else {
                    let found = match &self.heap.object(map_ref).kind {
                        ObjectKind::Set(s) => s.entries[start_idx..]
                            .iter()
                            .enumerate()
                            .find_map(|(off, e)| e.map(|val| (start_idx + off + 1, val))),
                        _ => None,
                    };
                    let Some((next_idx, v)) = found else {
                        return Ok(None);
                    };
                    if let ObjectKind::Iterator(IterKind::Set { idx, .. }) =
                        &mut self.heap.object_mut(iter_ref).kind
                    {
                        *idx = next_idx;
                    }
                    if mode_u == 5 {
                        let arr = self
                            .heap
                            .alloc_object(Some(self.protos.array), ObjectKind::Array(vec![v, v]));
                        Ok(Some(Value::object(arr)))
                    } else {
                        Ok(Some(v))
                    }
                }
            }
        }
    }

    pub(crate) fn collect_iterable(&mut self, src: Value) -> Result<Vec<Value>, Value> {
        // Caminho rápido para Arrays
        if let Some(o) = src.as_object()
            && let ObjectKind::Array(items) = &self.heap.object(o).kind
        {
            return Ok(items.clone());
        }
        let iter = self.make_iterator(src)?;
        let mut out = Vec::new();
        while let Some(v) = self.iter_next(iter)? {
            out.push(v);
        }
        Ok(out)
    }

    fn collect_for_in_keys(&mut self, obj_val: Value) -> GcRef {
        let mut keys: Vec<GcRef> = Vec::new();
        let mut seen: HashSet<GcRef> = HashSet::new();
        match obj_val.kind() {
            Kind::Undefined | Kind::Null => {
                return self
                    .heap
                    .alloc_object(Some(self.protos.iterator), ObjectKind::Iterator(IterKind::Empty));
            }
            Kind::String(s_ref) => {
                let len = self.heap.string(s_ref).encode_utf16().count();
                for i in 0..len {
                    let k = self.heap.intern(&i.to_string());
                    if seen.insert(k) {
                        keys.push(k);
                    }
                }
            }
            Kind::Object(o) => {
                let mut cur = Some(o);
                while let Some(obj_ref) = cur {
                    let mut local_keys = Vec::new();
                    let obj = self.heap.object(obj_ref);
                    if let ObjectKind::Array(items) = &obj.kind {
                        for i in 0..items.len() {
                            local_keys.push((true, i));
                        }
                    }
                    let prop_keys: Vec<GcRef> = obj
                        .props
                        .iter()
                        .filter(|p| p.flags & PROP_ENUMERABLE != 0)
                        .map(|p| p.key)
                        .collect();
                    let next = obj.proto;
                    for (_, i) in local_keys {
                        let k = self.heap.intern(&i.to_string());
                        if seen.insert(k) {
                            keys.push(k);
                        }
                    }
                    for k in prop_keys {
                        if seen.insert(k) {
                            keys.push(k);
                        }
                    }
                    cur = next;
                }
            }
            _ => {}
        }
        self.heap.alloc_object(
            Some(self.protos.iterator),
            ObjectKind::Iterator(IterKind::Keys { keys, idx: 0 }),
        )
    }

    fn object_assign_spread(&mut self, dst_obj: GcRef, src: Value) -> Result<(), Value> {
        match src.kind() {
            Kind::Undefined | Kind::Null => {}
            Kind::String(s_ref) => {
                let chars: Vec<String> = self
                    .heap
                    .string(s_ref)
                    .encode_utf16()
                    .map(|u| String::from_utf16_lossy(&[u]))
                    .collect();
                for (i, ch) in chars.into_iter().enumerate() {
                    let k = self.heap.intern(&i.to_string());
                    let sv = self.new_string(&ch);
                    self.heap.object_mut(dst_obj).set_own(k, sv);
                }
            }
            Kind::Object(o) => {
                let arr_items = match &self.heap.object(o).kind {
                    ObjectKind::Array(items) => Some(items.clone()),
                    _ => None,
                };
                if let Some(items) = arr_items {
                    for (i, v) in items.into_iter().enumerate() {
                        let k = self.heap.intern(&i.to_string());
                        self.heap.object_mut(dst_obj).set_own(k, v);
                    }
                }
                let props: Vec<(GcRef, Value)> = self
                    .heap
                    .object(o)
                    .props
                    .iter()
                    .filter(|p| p.flags & PROP_ENUMERABLE != 0)
                    .map(|p| (p.key, p.value))
                    .collect();
                for (k, v) in props {
                    self.heap.object_mut(dst_obj).set_own(k, v);
                }
            }
            _ => {}
        }
        Ok(())
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
            self.close_upvalues_from(f.base);
            self.regs.truncate(f.base);
        }
        Err(exc)
    }

    fn binary(&mut self, op: BinOp, a: Value, b: Value) -> Result<Value, Value> {
        use BinOp::*;
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
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

    fn invoke_call(
        &mut self,
        f: Value,
        this_v: Value,
        args_abs: Option<(usize, usize)>,
        args_slice: &[Value],
        dst_abs: usize,
    ) -> Result<(), Value> {
        let Some(o) = f.as_object() else {
            return Err(self.not_callable(f));
        };
        match &self.heap.object(o).kind {
            ObjectKind::Function(fn_obj) => {
                if fn_obj.code.is_class_ctor {
                    return Err(self.type_error("Class constructor cannot be invoked without 'new'"));
                }
                let code = fn_obj.code.clone();
                let uvs = fn_obj.upvalues.clone();
                let home = fn_obj.home_object;
                let sc = fn_obj.super_ctor;
                let eff_this = if code.is_arrow {
                    fn_obj.bound_this
                } else {
                    this_v
                };
                if let Some((abs, argc)) = args_abs {
                    self.push_frame_from_regs(
                        code,
                        Some(o),
                        uvs,
                        home,
                        sc,
                        eff_this,
                        abs,
                        argc,
                        Some(dst_abs),
                        None,
                    )
                } else {
                    self.push_frame(
                        code,
                        Some(o),
                        uvs,
                        home,
                        sc,
                        eff_this,
                        args_slice,
                        Some(dst_abs),
                        None,
                    )
                }
            }
            ObjectKind::Native { func, .. } => {
                let func = *func;
                let owned;
                let argv = if let Some((abs, argc)) = args_abs {
                    owned = self.regs[abs..abs + argc].to_vec();
                    &owned[..]
                } else {
                    args_slice
                };
                self.native_depth += 1;
                let res = func(self, this_v, argv);
                self.native_depth -= 1;
                self.regs[dst_abs] = res?;
                Ok(())
            }
            ObjectKind::HostFunction { func, .. } => {
                let func = func.clone();
                let owned;
                let argv = if let Some((abs, argc)) = args_abs {
                    owned = self.regs[abs..abs + argc].to_vec();
                    &owned[..]
                } else {
                    args_slice
                };
                self.native_depth += 1;
                let res = func(self, this_v, argv).map_err(|e| self.js_error_to_value(e));
                self.native_depth -= 1;
                self.regs[dst_abs] = res?;
                Ok(())
            }
            _ => Err(self.not_callable(f)),
        }
    }

    fn invoke_new(
        &mut self,
        f: Value,
        args_abs: Option<(usize, usize)>,
        args_slice: &[Value],
        dst_abs: usize,
    ) -> Result<(), Value> {
        let Some(o) = f.as_object() else {
            return Err(self.type_error("value is not a constructor"));
        };
        let proto_v = self.get_property(f, self.names.prototype)?;
        let proto = proto_v.as_object().unwrap_or(self.protos.object);
        match &self.heap.object(o).kind {
            ObjectKind::Function(fn_obj) if !fn_obj.code.is_arrow => {
                let code = fn_obj.code.clone();
                let uvs = fn_obj.upvalues.clone();
                let home = fn_obj.home_object;
                let sc = fn_obj.super_ctor;
                let obj = self.heap.alloc_object(Some(proto), ObjectKind::Ordinary);
                if let Some((abs, argc)) = args_abs {
                    self.push_frame_from_regs(
                        code,
                        Some(o),
                        uvs,
                        home,
                        sc,
                        Value::object(obj),
                        abs,
                        argc,
                        Some(dst_abs),
                        Some(obj),
                    )
                } else {
                    self.push_frame(
                        code,
                        Some(o),
                        uvs,
                        home,
                        sc,
                        Value::object(obj),
                        args_slice,
                        Some(dst_abs),
                        Some(obj),
                    )
                }
            }
            ObjectKind::Native {
                func,
                constructor: true,
            } => {
                let func = *func;
                let obj = self.heap.alloc_object(Some(proto), ObjectKind::Ordinary);
                let owned;
                let argv = if let Some((abs, argc)) = args_abs {
                    owned = self.regs[abs..abs + argc].to_vec();
                    &owned[..]
                } else {
                    args_slice
                };
                self.native_depth += 1;
                let res = func(self, Value::object(obj), argv);
                self.native_depth -= 1;
                let v = res?;
                self.regs[dst_abs] = if v.as_object().is_some() {
                    v
                } else {
                    Value::object(obj)
                };
                Ok(())
            }
            ObjectKind::HostFunction {
                func,
                constructor: true,
            } => {
                let func = func.clone();
                let obj = self.heap.alloc_object(Some(proto), ObjectKind::Ordinary);
                let owned;
                let argv = if let Some((abs, argc)) = args_abs {
                    owned = self.regs[abs..abs + argc].to_vec();
                    &owned[..]
                } else {
                    args_slice
                };
                self.native_depth += 1;
                let res = func(self, Value::object(obj), argv).map_err(|e| self.js_error_to_value(e));
                self.native_depth -= 1;
                let v = res?;
                self.regs[dst_abs] = if v.as_object().is_some() {
                    v
                } else {
                    Value::object(obj)
                };
                Ok(())
            }
            _ => Err(self.type_error("value is not a constructor")),
        }
    }

    fn invoke_super(
        &mut self,
        args_abs: Option<(usize, usize)>,
        args_slice: &[Value],
        dst_abs: usize,
    ) -> Result<(), Value> {
        let (super_ctor_opt, super_called, this_val, construct_opt) = {
            let f = self.frames.last().expect("frame");
            (f.super_ctor, f.super_called, f.this, f.construct)
        };
        let Some(sc) = super_ctor_opt else {
            return Err(self.type_error("super() is not valid in this context"));
        };
        if super_called {
            return Err(self.reference_error("Super constructor may only be called once"));
        }
        self.frames.last_mut().expect("frame").super_called = true;
        let derived_proto = construct_opt.and_then(|c| self.heap.object(c).proto);
        match &self.heap.object(sc).kind {
            ObjectKind::Function(fn_obj) if !fn_obj.code.is_arrow => {
                let code = fn_obj.code.clone();
                let uvs = fn_obj.upvalues.clone();
                let home = fn_obj.home_object;
                let next_sc = fn_obj.super_ctor;
                if let Some((abs, argc)) = args_abs {
                    self.push_frame_from_regs(
                        code,
                        Some(sc),
                        uvs,
                        home,
                        next_sc,
                        this_val,
                        abs,
                        argc,
                        Some(dst_abs),
                        construct_opt,
                    )
                } else {
                    self.push_frame(
                        code,
                        Some(sc),
                        uvs,
                        home,
                        next_sc,
                        this_val,
                        args_slice,
                        Some(dst_abs),
                        construct_opt,
                    )
                }
            }
            ObjectKind::Native {
                func,
                constructor: true,
            } => {
                let func = *func;
                let owned;
                let argv = if let Some((abs, argc)) = args_abs {
                    owned = self.regs[abs..abs + argc].to_vec();
                    &owned[..]
                } else {
                    args_slice
                };
                self.native_depth += 1;
                let res = func(self, this_val, argv);
                self.native_depth -= 1;
                let v = res?;
                let out = if let Some(new_obj) = v.as_object() {
                    if let Some(dp) = derived_proto {
                        self.heap.object_mut(new_obj).proto = Some(dp);
                    }
                    let frame = self.frames.last_mut().expect("frame");
                    frame.this = v;
                    frame.construct = Some(new_obj);
                    v
                } else {
                    this_val
                };
                self.regs[dst_abs] = out;
                Ok(())
            }
            _ => Err(self.type_error("Super constructor is not constructible")),
        }
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
                    let f = self.frames.last().expect("frame");
                    if f.code.is_derived_ctor && !f.super_called {
                        return Err(self.reference_error(
                            "Must call super constructor in derived class before accessing 'this'",
                        ));
                    }
                    self.regs[r(dst)] = f.this;
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
                Op::JumpIfNullish { cond, target } => {
                    if self.regs[r(cond)].is_nullish() {
                        self.jump(target);
                    }
                }
                Op::JumpIfNotUndefined { cond, target } => {
                    if !self.regs[r(cond)].is_undefined() {
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
                Op::DeleteGlobal { dst, name } => {
                    let key = self.const_name(name);
                    let ok = self.heap.object_mut(self.global).delete_own(key);
                    self.regs[r(dst)] = Value::boolean(ok);
                }
                Op::GetUpvalue { dst, upvalue } => {
                    let uv = self.frames.last().expect("frame").upvalues[upvalue as usize];
                    let val = match self.heap.upvalue(uv) {
                        UpvalueState::Open(abs) => self.regs[abs],
                        UpvalueState::Closed(v) => v,
                    };
                    self.regs[r(dst)] = val;
                }
                Op::SetUpvalue { upvalue, src } => {
                    let val = self.regs[r(src)];
                    let uv = self.frames.last().expect("frame").upvalues[upvalue as usize];
                    match self.heap.upvalue(uv) {
                        UpvalueState::Open(abs) => self.regs[abs] = val,
                        UpvalueState::Closed(_) => self.heap.set_closed_upvalue(uv, val),
                    }
                }
                Op::CloseUpvalues { from_reg } => {
                    self.close_upvalues_from(r(from_reg));
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
                Op::DeleteNamed { dst, obj, name } => {
                    let key = self.const_name(name);
                    let ok = self.delete_property(self.regs[r(obj)], key)?;
                    self.regs[r(dst)] = Value::boolean(ok);
                }
                Op::GetKeyed { dst, obj, key } => {
                    let v = self.get_keyed(self.regs[r(obj)], self.regs[r(key)])?;
                    self.regs[r(dst)] = v;
                }
                Op::SetKeyed { obj, key, src } => {
                    self.set_keyed(self.regs[r(obj)], self.regs[r(key)], self.regs[r(src)])?;
                }
                Op::DeleteKeyed { dst, obj, key } => {
                    let k = self.intern_value(self.regs[r(key)])?;
                    let ok = self.delete_property(self.regs[r(obj)], k)?;
                    self.regs[r(dst)] = Value::boolean(ok);
                }
                Op::CheckObjectNotNullish { src } => {
                    if self.regs[r(src)].is_nullish() {
                        return Err(self.type_error("Cannot destructure 'undefined' or 'null'"));
                    }
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
                Op::ArrayPushSpread { array, iterable } => {
                    let items_to_add = self.collect_iterable(self.regs[r(iterable)])?;
                    if let Some(o) = self.regs[r(array)].as_object()
                        && let ObjectKind::Array(items) = &mut self.heap.object_mut(o).kind
                    {
                        items.extend(items_to_add);
                    }
                }
                Op::ObjectAssignSpread { obj, src } => {
                    if let Some(dst_obj) = self.regs[r(obj)].as_object() {
                        self.object_assign_spread(dst_obj, self.regs[r(src)])?;
                    }
                }
                Op::ObjectRest {
                    dst,
                    obj,
                    excl_start,
                    excl_count,
                } => {
                    let src_val = self.regs[r(obj)];
                    if src_val.is_nullish() {
                        return Err(self.type_error("Cannot destructure 'undefined' or 'null'"));
                    }
                    let mut excluded = HashSet::new();
                    for i in 0..excl_count as usize {
                        let k = self.intern_value(self.regs[r(excl_start) + i])?;
                        excluded.insert(k);
                    }
                    let new_obj = self
                        .heap
                        .alloc_object(Some(self.protos.object), ObjectKind::Ordinary);
                    self.object_assign_spread(new_obj, src_val)?;
                    self.heap
                        .object_mut(new_obj)
                        .props
                        .retain(|p| !excluded.contains(&p.key));
                    self.regs[r(dst)] = Value::object(new_obj);
                }
                Op::DefineMethod {
                    obj,
                    name,
                    func,
                    enumerable,
                } => {
                    let key = self.const_name(name);
                    let fn_val = self.regs[r(func)];
                    if let Some(target) = self.regs[r(obj)].as_object() {
                        if let Some(fo) = fn_val.as_object()
                            && let ObjectKind::Function(ref mut fn_obj) =
                                self.heap.object_mut(fo).kind
                        {
                            fn_obj.home_object = Some(target);
                        }
                        let flags = if enumerable {
                            crate::heap::PROP_DEFAULT
                        } else {
                            PROP_METHOD
                        };
                        self.heap.object_mut(target).define_own(key, fn_val, flags);
                    }
                }
                Op::DefineMethodKeyed {
                    obj,
                    key,
                    func,
                    enumerable,
                } => {
                    let key_ref = self.intern_value(self.regs[r(key)])?;
                    let fn_val = self.regs[r(func)];
                    if let Some(target) = self.regs[r(obj)].as_object() {
                        if let Some(fo) = fn_val.as_object()
                            && let ObjectKind::Function(ref mut fn_obj) =
                                self.heap.object_mut(fo).kind
                        {
                            fn_obj.home_object = Some(target);
                        }
                        let flags = if enumerable {
                            crate::heap::PROP_DEFAULT
                        } else {
                            PROP_METHOD
                        };
                        self.heap
                            .object_mut(target)
                            .define_own(key_ref, fn_val, flags);
                    }
                }
                Op::SetupClass { ctor, super_ctor } => {
                    let ctor_val = self.regs[r(ctor)];
                    let super_val = self.regs[r(super_ctor)];
                    let Some(ctor_ref) = ctor_val.as_object() else {
                        return Err(self.type_error("Class constructor must be an object"));
                    };
                    let ctor_proto_val = self.get_property(ctor_val, self.names.prototype)?;
                    let ctor_proto_ref = ctor_proto_val.as_object().expect("ctor prototype");

                    match super_val.kind() {
                        Kind::Undefined => {
                            // Classe base: ctor.prototype.__proto__ = Object.prototype
                        }
                        Kind::Null => {
                            self.heap.object_mut(ctor_proto_ref).proto = None;
                        }
                        Kind::Object(sc_ref) => {
                            let constructible = match &self.heap.object(sc_ref).kind {
                                ObjectKind::Function(f) => !f.code.is_arrow,
                                ObjectKind::Native { constructor, .. }
                                | ObjectKind::HostFunction { constructor, .. } => *constructor,
                                _ => false,
                            };
                            if !constructible {
                                return Err(self.type_error(
                                    "Class extends value is not a constructor or null",
                                ));
                            }
                            let super_proto_val =
                                self.get_property(super_val, self.names.prototype)?;
                            let super_proto_opt = match super_proto_val.kind() {
                                Kind::Object(p) => Some(p),
                                Kind::Null => None,
                                _ => {
                                    return Err(self.type_error(
                                        "Class extends value does not have valid prototype",
                                    ));
                                }
                            };
                            self.heap.object_mut(ctor_proto_ref).proto = super_proto_opt;
                            let ctor_obj = self.heap.object_mut(ctor_ref);
                            ctor_obj.proto = Some(sc_ref);
                            if let ObjectKind::Function(ref mut fn_obj) = ctor_obj.kind {
                                fn_obj.super_ctor = Some(sc_ref);
                                fn_obj.home_object = Some(ctor_proto_ref);
                            }
                        }
                        _ => {
                            return Err(
                                self.type_error("Class extends value is not a constructor or null")
                            );
                        }
                    }
                }
                Op::ForInInit { dst, obj } => {
                    self.maybe_gc();
                    let it = self.collect_for_in_keys(self.regs[r(obj)]);
                    self.regs[r(dst)] = Value::object(it);
                }
                Op::ForOfInit { dst, src } => {
                    self.maybe_gc();
                    let it = self.make_iterator(self.regs[r(src)])?;
                    self.regs[r(dst)] = Value::object(it);
                }
                Op::IterNext {
                    iter,
                    dst,
                    done_target,
                } => {
                    let Some(iter_ref) = self.regs[r(iter)].as_object() else {
                        return Err(self.type_error("expected iterator"));
                    };
                    match self.iter_next(iter_ref)? {
                        Some(v) => self.regs[r(dst)] = v,
                        None => {
                            self.regs[r(dst)] = Value::UNDEFINED;
                            self.jump(done_target);
                        }
                    }
                }
                Op::IterRest { iter, dst } => {
                    let Some(iter_ref) = self.regs[r(iter)].as_object() else {
                        return Err(self.type_error("expected iterator"));
                    };
                    let mut rest = Vec::new();
                    while let Some(v) = self.iter_next(iter_ref)? {
                        rest.push(v);
                    }
                    let arr = self
                        .heap
                        .alloc_object(Some(self.protos.array), ObjectKind::Array(rest));
                    self.regs[r(dst)] = Value::object(arr);
                }
                Op::Closure { dst, func } => {
                    self.maybe_gc();
                    let (code, caller_base, caller_uvs, caller_this, caller_home, caller_super) = {
                        let frame = self.frames.last().expect("frame");
                        (
                            frame.code.functions[func as usize].clone(),
                            frame.base,
                            frame.upvalues.clone(),
                            frame.this,
                            frame.home_object,
                            frame.super_ctor,
                        )
                    };
                    let mut uvs = Vec::with_capacity(code.upvalues.len());
                    for desc in &code.upvalues {
                        let uv = if desc.is_local {
                            self.capture_upvalue(caller_base + desc.index as usize)
                        } else {
                            caller_uvs[desc.index as usize]
                        };
                        uvs.push(uv);
                    }
                    let bound_this = if code.is_arrow {
                        caller_this
                    } else {
                        Value::UNDEFINED
                    };
                    let (home_object, super_ctor) = if code.is_arrow {
                        (caller_home, caller_super)
                    } else {
                        (None, None)
                    };
                    let f = self.make_closure(
                        code,
                        uvs.into(),
                        bound_this,
                        home_object,
                        super_ctor,
                    );
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
                    self.invoke_call(f, this_v, Some((r(args), argc as usize)), &[], r(dst))?;
                }
                Op::CallSpread {
                    dst,
                    callee,
                    this,
                    args_array,
                } => {
                    self.maybe_gc();
                    let f = self.regs[r(callee)];
                    let this_v = self.regs[r(this)];
                    let argv = match self.regs[r(args_array)].as_object() {
                        Some(o) => match &self.heap.object(o).kind {
                            ObjectKind::Array(items) => items.clone(),
                            _ => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    self.invoke_call(f, this_v, None, &argv, r(dst))?;
                }
                Op::New {
                    dst,
                    callee,
                    args,
                    argc,
                } => {
                    self.maybe_gc();
                    let f = self.regs[r(callee)];
                    self.invoke_new(f, Some((r(args), argc as usize)), &[], r(dst))?;
                }
                Op::NewSpread {
                    dst,
                    callee,
                    args_array,
                } => {
                    self.maybe_gc();
                    let f = self.regs[r(callee)];
                    let argv = match self.regs[r(args_array)].as_object() {
                        Some(o) => match &self.heap.object(o).kind {
                            ObjectKind::Array(items) => items.clone(),
                            _ => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    self.invoke_new(f, None, &argv, r(dst))?;
                }
                Op::SuperCall { dst, args, argc } => {
                    self.maybe_gc();
                    self.invoke_super(Some((r(args), argc as usize)), &[], r(dst))?;
                }
                Op::SuperCallSpread { dst, args_array } => {
                    self.maybe_gc();
                    let argv = match self.regs[r(args_array)].as_object() {
                        Some(o) => match &self.heap.object(o).kind {
                            ObjectKind::Array(items) => items.clone(),
                            _ => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    self.invoke_super(None, &argv, r(dst))?;
                }
                Op::FinishSuper { src } => {
                    let v = self.regs[r(src)];
                    let frame = self.frames.last_mut().expect("frame");
                    if let Some(o) = v.as_object() {
                        frame.this = v;
                        frame.construct = Some(o);
                    }
                }
                Op::GetSuperNamed { dst, name } => {
                    let key = self.const_name(name);
                    let home = self.frames.last().expect("frame").home_object;
                    let Some(home_ref) = home else {
                        return Err(self.type_error("super property access outside method"));
                    };
                    let val = match self.heap.object(home_ref).proto {
                        Some(super_proto) => {
                            self.lookup(super_proto, key).unwrap_or(Value::UNDEFINED)
                        }
                        None => Value::UNDEFINED,
                    };
                    self.regs[r(dst)] = val;
                }
                Op::GetSuperKeyed { dst, key } => {
                    let key_ref = self.intern_value(self.regs[r(key)])?;
                    let home = self.frames.last().expect("frame").home_object;
                    let Some(home_ref) = home else {
                        return Err(self.type_error("super property access outside method"));
                    };
                    let val = match self.heap.object(home_ref).proto {
                        Some(super_proto) => self
                            .lookup(super_proto, key_ref)
                            .unwrap_or(Value::UNDEFINED),
                        None => Value::UNDEFINED,
                    };
                    self.regs[r(dst)] = val;
                }
                Op::Return { src } => {
                    let v = self.regs[r(src)];
                    let frame = self.frames.pop().expect("frame");
                    self.close_upvalues_from(frame.base);
                    self.regs.truncate(frame.base);
                    if frame.code.is_derived_ctor && !frame.super_called && v.as_object().is_none()
                    {
                        return Err(self.reference_error(
                            "Must call super constructor in derived class before returning",
                        ));
                    }
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

fn push_code_constants(code: &FunctionCode, roots: &mut Vec<Value>) {
    roots.extend_from_slice(&code.constants);
    for f in &code.functions {
        push_code_constants(f, roots);
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
