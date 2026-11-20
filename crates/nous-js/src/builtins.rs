//! Objetos e funções nativas mínimas (crescem guiadas pelo test262).

use crate::heap::{GcRef, ObjectKind};
use crate::value::{Kind, Value};
use crate::vm::{NativeFn, Vm, number_to_string, string_to_number, to_int32};

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).copied().unwrap_or(Value::UNDEFINED)
}

fn define(vm: &mut Vm, target: GcRef, name: &str, value: Value) {
    let key = vm.heap.intern(name);
    vm.heap.object_mut(target).set_own(key, value);
}

fn native(vm: &mut Vm, name: &str, func: NativeFn, length: u32, constructor: bool) -> GcRef {
    let f = vm.heap.alloc_object(
        Some(vm.protos.function),
        ObjectKind::Native { func, constructor },
    );
    let n = vm.heap.intern(name);
    let (k_name, k_len) = (vm.names.name, vm.names.length);
    let obj = vm.heap.object_mut(f);
    obj.set_own(k_name, Value::string(n));
    obj.set_own(k_len, Value::number(length as f64));
    f
}

fn method(vm: &mut Vm, target: GcRef, name: &str, func: NativeFn, length: u32) {
    let f = native(vm, name, func, length, false);
    define(vm, target, name, Value::object(f));
}

/// Cria um construtor nativo ligado ao seu protótipo (`F.prototype` e `proto.constructor`).
fn constructor(vm: &mut Vm, name: &str, func: NativeFn, length: u32, proto: GcRef) -> GcRef {
    let f = native(vm, name, func, length, true);
    let (k_proto, k_ctor) = (vm.names.prototype, vm.names.constructor);
    vm.heap.object_mut(f).set_own(k_proto, Value::object(proto));
    vm.heap.object_mut(proto).set_own(k_ctor, Value::object(f));
    define(vm, vm.global, name, Value::object(f));
    f
}

pub fn install(vm: &mut Vm) {
    let global = vm.global;
    define(vm, global, "globalThis", Value::object(global));
    define(vm, global, "undefined", Value::UNDEFINED);
    define(vm, global, "NaN", Value::number(f64::NAN));
    define(vm, global, "Infinity", Value::number(f64::INFINITY));
    method(vm, global, "print", print, 1);
    method(vm, global, "isNaN", is_nan, 1);
    method(vm, global, "isFinite", is_finite, 1);
    method(vm, global, "parseInt", parse_int, 2);
    method(vm, global, "parseFloat", parse_float, 1);

    let console = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    method(vm, console, "log", print, 0);
    method(vm, console, "error", print, 0);
    define(vm, global, "console", Value::object(console));

    // Object
    let object_proto = vm.protos.object;
    let object_ctor = constructor(vm, "Object", object_ctor, 1, object_proto);
    method(vm, object_proto, "toString", object_to_string, 0);
    method(vm, object_proto, "valueOf", object_value_of, 0);
    method(vm, object_proto, "hasOwnProperty", object_has_own, 1);
    method(
        vm,
        object_ctor,
        "getPrototypeOf",
        object_get_prototype_of,
        1,
    );
    method(vm, object_ctor, "keys", object_keys, 1);
    method(vm, object_ctor, "is", object_is, 2);

    // Function
    let function_proto = vm.protos.function;
    constructor(vm, "Function", function_ctor, 1, function_proto);
    method(vm, function_proto, "call", function_call, 1);
    method(vm, function_proto, "apply", function_apply, 2);
    method(vm, function_proto, "toString", function_to_string, 0);

    // Array
    let array_proto = vm.protos.array;
    let array_ctor = constructor(vm, "Array", array_ctor, 1, array_proto);
    method(vm, array_ctor, "isArray", array_is_array, 1);
    method(vm, array_proto, "push", array_push, 1);
    method(vm, array_proto, "join", array_join, 1);
    method(vm, array_proto, "toString", array_to_string, 0);

    // Primitivos
    let string_proto = vm.protos.string;
    constructor(vm, "String", string_ctor, 1, string_proto);
    let number_proto = vm.protos.number;
    constructor(vm, "Number", number_ctor, 1, number_proto);
    method(vm, number_proto, "toString", number_to_string_method, 1);
    let boolean_proto = vm.protos.boolean;
    constructor(vm, "Boolean", boolean_ctor, 1, boolean_proto);

    // Erros
    let error_proto = vm.protos.error;
    constructor(vm, "Error", error_ctor::<0>, 1, error_proto);
    method(vm, error_proto, "toString", error_to_string, 0);
    for (kind, name, proto) in [
        (1u8, "TypeError", vm.protos.type_error),
        (2, "ReferenceError", vm.protos.reference_error),
        (3, "RangeError", vm.protos.range_error),
        (4, "SyntaxError", vm.protos.syntax_error),
    ] {
        let ctor: NativeFn = match kind {
            1 => error_ctor::<1>,
            2 => error_ctor::<2>,
            3 => error_ctor::<3>,
            _ => error_ctor::<4>,
        };
        constructor(vm, name, ctor, 1, proto);
    }
    for (name, proto) in [
        ("Error", vm.protos.error),
        ("TypeError", vm.protos.type_error),
        ("ReferenceError", vm.protos.reference_error),
        ("RangeError", vm.protos.range_error),
        ("SyntaxError", vm.protos.syntax_error),
    ] {
        let n = vm.heap.intern(name);
        let empty = vm.heap.intern("");
        let (k_name, k_msg) = (vm.names.name, vm.names.message);
        let p = vm.heap.object_mut(proto);
        p.set_own(k_name, Value::string(n));
        p.set_own(k_msg, Value::string(empty));
    }

    // Math
    let math = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    define(vm, global, "Math", Value::object(math));
    define(vm, math, "PI", Value::number(std::f64::consts::PI));
    define(vm, math, "E", Value::number(std::f64::consts::E));
    method(vm, math, "abs", |vm, _, a| math1(vm, a, f64::abs), 1);
    method(vm, math, "floor", |vm, _, a| math1(vm, a, f64::floor), 1);
    method(vm, math, "ceil", |vm, _, a| math1(vm, a, f64::ceil), 1);
    method(vm, math, "trunc", |vm, _, a| math1(vm, a, f64::trunc), 1);
    method(vm, math, "sqrt", |vm, _, a| math1(vm, a, f64::sqrt), 1);
    method(vm, math, "sign", |vm, _, a| math1(vm, a, js_sign), 1);
    method(vm, math, "round", |vm, _, a| math1(vm, a, js_round), 1);
    method(vm, math, "pow", math_pow, 2);
    method(
        vm,
        math,
        "max",
        |vm, _, a| math_fold(vm, a, f64::NEG_INFINITY, true),
        2,
    );
    method(
        vm,
        math,
        "min",
        |vm, _, a| math_fold(vm, a, f64::INFINITY, false),
        2,
    );
    method(vm, math, "random", math_random, 0);
}

// ------------------------------------------------------------------ globais

fn print(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let mut parts = Vec::with_capacity(args.len());
    for a in args {
        parts.push(vm.display(*a));
    }
    let line = parts.join(" ");
    (vm.output)(&line);
    Ok(Value::UNDEFINED)
}

fn is_nan(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.to_number(arg(args, 0))?.is_nan()))
}

fn is_finite(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.to_number(arg(args, 0))?.is_finite()))
}

fn parse_int(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let s = vm.to_str(arg(args, 0))?;
    let mut t = s.trim_start();
    let neg = t.starts_with('-');
    t = t.trim_start_matches(['-', '+']);
    let mut radix = match arg(args, 1).kind() {
        Kind::Undefined => 0,
        _ => to_int32(vm.to_number(arg(args, 1))?),
    };
    if (radix == 0 || radix == 16)
        && let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
    {
        t = rest;
        radix = 16;
    }
    if radix == 0 {
        radix = 10;
    }
    if !(2..=36).contains(&radix) {
        return Ok(Value::number(f64::NAN));
    }
    let digits: String = t.chars().take_while(|c| c.is_digit(radix as u32)).collect();
    if digits.is_empty() {
        return Ok(Value::number(f64::NAN));
    }
    let mut n = 0f64;
    for c in digits.chars() {
        n = n * radix as f64 + c.to_digit(radix as u32).unwrap_or(0) as f64;
    }
    Ok(Value::number(if neg { -n } else { n }))
}

fn parse_float(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let s = vm.to_str(arg(args, 0))?;
    let t = s.trim_start();
    if t.starts_with("Infinity") || t.starts_with("+Infinity") {
        return Ok(Value::number(f64::INFINITY));
    }
    if t.starts_with("-Infinity") {
        return Ok(Value::number(f64::NEG_INFINITY));
    }
    // maior prefixo que é um número válido
    let mut best = f64::NAN;
    for end in (1..=t.len()).rev() {
        if !t.is_char_boundary(end) {
            continue;
        }
        let candidate = &t[..end];
        if candidate.ends_with(['e', 'E', '+', '-']) {
            continue;
        }
        let n = string_to_number(candidate);
        if !n.is_nan() && !candidate.starts_with("0x") {
            best = n;
            break;
        }
    }
    Ok(Value::number(best))
}

// ------------------------------------------------------------------- Object

fn object_ctor(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let a = arg(args, 0);
    if a.as_object().is_some() {
        return Ok(a);
    }
    if this.as_object().is_some_and(|o| o != vm.global) {
        return Ok(this);
    }
    let o = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    Ok(Value::object(o))
}

fn object_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let tag = match this.kind() {
        Kind::Undefined => "[object Undefined]",
        Kind::Null => "[object Null]",
        Kind::Object(o) => match vm.heap.object(o).kind {
            ObjectKind::Array(_) => "[object Array]",
            ObjectKind::Function(_) | ObjectKind::Native { .. } => "[object Function]",
            ObjectKind::Error => "[object Error]",
            _ => "[object Object]",
        },
        Kind::Number(_) => "[object Number]",
        Kind::String(_) => "[object String]",
        Kind::Bool(_) => "[object Boolean]",
    };
    Ok(Value::string(vm.heap.intern(tag)))
}

fn object_value_of(_: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    Ok(this)
}

fn object_has_own(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let key = vm.intern_value(arg(args, 0))?;
    let Some(o) = this.as_object() else {
        return Ok(Value::FALSE);
    };
    let obj = vm.heap.object(o);
    let mut has = obj.get_own(key).is_some();
    if let ObjectKind::Array(items) = &obj.kind {
        has |= key == vm.names.length
            || crate::vm::array_index(vm.heap.string(key)).is_some_and(|i| i < items.len());
    }
    Ok(Value::boolean(has))
}

fn object_get_prototype_of(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    match arg(args, 0).as_object() {
        Some(o) => Ok(vm
            .heap
            .object(o)
            .proto
            .map(Value::object)
            .unwrap_or(Value::NULL)),
        None => Err(vm.type_error("Object.getPrototypeOf called on non-object")),
    }
}

fn object_keys(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(o) = arg(args, 0).as_object() else {
        return Err(vm.type_error("Object.keys called on non-object"));
    };
    let mut keys: Vec<Value> = Vec::new();
    let len = match &vm.heap.object(o).kind {
        ObjectKind::Array(items) => items.len(),
        _ => 0,
    };
    for i in 0..len {
        let s = vm.heap.alloc_string(&i.to_string());
        keys.push(Value::string(s));
    }
    keys.extend(
        vm.heap
            .object(o)
            .props
            .iter()
            .map(|(k, _)| Value::string(*k)),
    );
    let arr = vm
        .heap
        .alloc_object(Some(vm.protos.array), ObjectKind::Array(keys));
    Ok(Value::object(arr))
}

fn object_is(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.same_value(arg(args, 0), arg(args, 1))))
}

// ----------------------------------------------------------------- Function

fn function_ctor(vm: &mut Vm, _: Value, _: &[Value]) -> Result<Value, Value> {
    Err(vm.type_error("Function constructor ainda não suportado"))
}

fn function_call(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let rest = if args.len() > 1 { &args[1..] } else { &[] };
    vm.call_function(this, arg(args, 0), rest)
}

fn function_apply(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let list = match arg(args, 1).as_object() {
        Some(o) => match &vm.heap.object(o).kind {
            ObjectKind::Array(items) => items.clone(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    vm.call_function(this, arg(args, 0), &list)
}

fn function_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    if !vm.is_callable(this) {
        return Err(vm.type_error("Function.prototype.toString requires a function"));
    }
    let name = vm.get_property(this, vm.names.name)?;
    let name = vm.to_str(name)?;
    Ok(vm.new_string(&format!("function {name}() {{ [native code] }}")))
}

// -------------------------------------------------------------------- Array

fn array_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let items = if let [len] = args
        && let Some(n) = len.as_number()
    {
        if n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
            return Err(vm.range_error("Invalid array length"));
        }
        vec![Value::UNDEFINED; n as usize]
    } else {
        args.to_vec()
    };
    let o = vm
        .heap
        .alloc_object(Some(vm.protos.array), ObjectKind::Array(items));
    Ok(Value::object(o))
}

fn array_is_array(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(arg(args, 0).as_object().is_some_and(|o| {
        matches!(vm.heap.object(o).kind, ObjectKind::Array(_))
    })))
}

fn array_push(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Array.prototype.push called on non-object"));
    };
    match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(items) => {
            items.extend_from_slice(args);
            Ok(Value::number(items.len() as f64))
        }
        _ => Err(vm.type_error("Array.prototype.push em objeto genérico ainda não suportado")),
    }
}

fn array_join(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let sep = match arg(args, 0).kind() {
        Kind::Undefined => ",".into(),
        _ => vm.to_str(arg(args, 0))?,
    };
    let items = match this.as_object().map(|o| &vm.heap.object(o).kind) {
        Some(ObjectKind::Array(items)) => items.clone(),
        _ => return Ok(vm.new_string("")),
    };
    let mut out = String::new();
    for (i, v) in items.into_iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        if !v.is_nullish() {
            out.push_str(&vm.to_str(v)?);
        }
    }
    Ok(vm.new_string(&out))
}

fn array_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    array_join(vm, this, &[])
}

// --------------------------------------------------------------- primitivos

fn string_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::string(vm.heap.intern("")));
    }
    let s = vm.to_str(args[0])?;
    Ok(vm.new_string(&s))
}

fn number_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::number(0.0));
    }
    Ok(Value::number(vm.to_number(args[0])?))
}

fn number_to_string_method(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    match this.as_number() {
        Some(n) => Ok(vm.new_string(&number_to_string(n))),
        None => Err(vm.type_error("Number.prototype.toString requires a number")),
    }
}

fn boolean_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.to_boolean(arg(args, 0))))
}

// -------------------------------------------------------------------- erros

fn error_ctor<const K: u8>(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let proto = match K {
        1 => vm.protos.type_error,
        2 => vm.protos.reference_error,
        3 => vm.protos.range_error,
        4 => vm.protos.syntax_error,
        _ => vm.protos.error,
    };
    // chamado com `new`: `this` já é o objeto novo; chamado direto: cria um
    let obj = match this.as_object() {
        Some(o) if o != vm.global => {
            vm.heap.object_mut(o).kind = ObjectKind::Error;
            o
        }
        _ => vm.heap.alloc_object(Some(proto), ObjectKind::Error),
    };
    let msg = arg(args, 0);
    if !msg.is_undefined() {
        let s = vm.to_str(msg)?;
        let sv = vm.new_string(&s);
        let k = vm.names.message;
        vm.heap.object_mut(obj).set_own(k, sv);
    }
    Ok(Value::object(obj))
}

fn error_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    if this.as_object().is_none() {
        return Err(vm.type_error("Error.prototype.toString called on non-object"));
    }
    let name = vm.get_property(this, vm.names.name)?;
    let name = if name.is_undefined() {
        "Error".into()
    } else {
        vm.to_str(name)?
    };
    let msg = vm.get_property(this, vm.names.message)?;
    let msg = if msg.is_undefined() {
        "".into()
    } else {
        vm.to_str(msg)?
    };
    let s = match (name.is_empty(), msg.is_empty()) {
        (_, true) => name.to_string(),
        (true, false) => msg.to_string(),
        _ => format!("{name}: {msg}"),
    };
    Ok(vm.new_string(&s))
}

// --------------------------------------------------------------------- Math

fn math1(vm: &mut Vm, args: &[Value], f: fn(f64) -> f64) -> Result<Value, Value> {
    Ok(Value::number(f(vm.to_number(arg(args, 0))?)))
}

fn js_sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else {
        x.signum()
    }
}

fn js_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let r = (x + 0.5).floor();
    if r == 0.0 && x < 0.0 { -0.0 } else { r }
}

fn math_pow(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let x = vm.to_number(arg(args, 0))?;
    let y = vm.to_number(arg(args, 1))?;
    if y.is_nan() || (x.abs() == 1.0 && y.is_infinite()) {
        return Ok(Value::number(f64::NAN));
    }
    Ok(Value::number(x.powf(y)))
}

fn math_fold(vm: &mut Vm, args: &[Value], init: f64, max: bool) -> Result<Value, Value> {
    let mut acc = init;
    for a in args {
        let n = vm.to_number(*a)?;
        if n.is_nan() {
            acc = f64::NAN;
        } else if !acc.is_nan() {
            let better = if max {
                n > acc || (n == 0.0 && acc == 0.0 && !n.is_sign_negative())
            } else {
                n < acc || (n == 0.0 && acc == 0.0 && n.is_sign_negative())
            };
            if better {
                acc = n;
            }
        }
    }
    Ok(Value::number(acc))
}

fn math_random(vm: &mut Vm, _: Value, _: &[Value]) -> Result<Value, Value> {
    // xorshift64*: rápido e sem dependências; não serve para criptografia
    let mut x = vm.rng_state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    vm.rng_state = x;
    let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
    Ok(Value::number(r as f64 / (1u64 << 53) as f64))
}
