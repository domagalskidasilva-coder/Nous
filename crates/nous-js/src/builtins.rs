//! Objetos e funções nativas padrão do JavaScript.

use std::cmp::Ordering;

use crate::heap::{
    GcRef, IterKind, JsMap, JsSet, MapIterMode, MapKey, ObjectKind, PROP_CONFIGURABLE,
    PROP_ENUMERABLE, PROP_METHOD, PROP_WRITABLE,
};
use crate::json;
use crate::value::{Kind, Value};
use crate::vm::{NativeFn, Vm, array_index, number_to_string, string_to_number, to_int32};

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).copied().unwrap_or(Value::UNDEFINED)
}

fn define(vm: &mut Vm, target: GcRef, name: &str, value: Value) {
    let key = vm.heap.intern(name);
    vm.heap.object_mut(target).define_own(key, value, PROP_METHOD);
}

fn method(vm: &mut Vm, target: GcRef, name: &str, func: NativeFn, length: u32) {
    let f = vm.alloc_native_fn(name, func, length, false);
    define(vm, target, name, Value::object(f));
}

fn constructor(vm: &mut Vm, name: &str, func: NativeFn, length: u32, proto: GcRef) -> GcRef {
    let f = vm.alloc_native_fn(name, func, length, true);
    let (k_proto, k_ctor) = (vm.names.prototype, vm.names.constructor);
    vm.heap
        .object_mut(f)
        .define_own(k_proto, Value::object(proto), PROP_METHOD);
    vm.heap
        .object_mut(proto)
        .define_own(k_ctor, Value::object(f), PROP_METHOD);
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
    method(vm, console, "warn", print, 0);
    define(vm, global, "console", Value::object(console));

    // Object
    let object_proto = vm.protos.object;
    let object_ctor = constructor(vm, "Object", object_ctor, 1, object_proto);
    method(vm, object_proto, "toString", object_to_string, 0);
    method(vm, object_proto, "valueOf", object_value_of, 0);
    method(vm, object_proto, "hasOwnProperty", object_has_own_prop, 1);
    method(vm, object_ctor, "getPrototypeOf", object_get_prototype_of, 1);
    method(vm, object_ctor, "setPrototypeOf", object_set_prototype_of, 2);
    method(vm, object_ctor, "keys", object_keys, 1);
    method(vm, object_ctor, "values", object_values, 1);
    method(vm, object_ctor, "entries", object_entries, 1);
    method(vm, object_ctor, "fromEntries", object_from_entries, 1);
    method(vm, object_ctor, "assign", object_assign, 2);
    method(vm, object_ctor, "create", object_create, 2);
    method(vm, object_ctor, "hasOwn", object_has_own, 2);
    method(vm, object_ctor, "freeze", object_freeze, 1);
    method(vm, object_ctor, "isFrozen", object_is_frozen, 1);
    method(vm, object_ctor, "is", object_is, 2);
    method(vm, object_ctor, "defineProperty", object_define_property, 3);
    method(
        vm,
        object_ctor,
        "getOwnPropertyNames",
        object_get_own_property_names,
        1,
    );

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
    method(vm, array_ctor, "from", array_from, 1);
    method(vm, array_ctor, "of", array_of, 0);
    method(vm, array_proto, "push", array_push, 1);
    method(vm, array_proto, "pop", array_pop, 0);
    method(vm, array_proto, "shift", array_shift, 0);
    method(vm, array_proto, "unshift", array_unshift, 1);
    method(vm, array_proto, "slice", array_slice, 2);
    method(vm, array_proto, "splice", array_splice, 2);
    method(vm, array_proto, "concat", array_concat, 1);
    method(vm, array_proto, "join", array_join, 1);
    method(vm, array_proto, "indexOf", array_index_of, 1);
    method(vm, array_proto, "lastIndexOf", array_last_index_of, 1);
    method(vm, array_proto, "includes", array_includes, 1);
    method(vm, array_proto, "reverse", array_reverse, 0);
    method(vm, array_proto, "map", array_map, 1);
    method(vm, array_proto, "filter", array_filter, 1);
    method(vm, array_proto, "reduce", array_reduce, 1);
    method(vm, array_proto, "reduceRight", array_reduce_right, 1);
    method(vm, array_proto, "forEach", array_for_each, 1);
    method(vm, array_proto, "find", array_find, 1);
    method(vm, array_proto, "findIndex", array_find_index, 1);
    method(vm, array_proto, "some", array_some, 1);
    method(vm, array_proto, "every", array_every, 1);
    method(vm, array_proto, "flat", array_flat, 0);
    method(vm, array_proto, "sort", array_sort, 1);
    method(vm, array_proto, "fill", array_fill, 1);
    method(vm, array_proto, "toString", array_to_string, 0);
    method(vm, array_proto, "keys", array_keys_iter, 0);
    method(vm, array_proto, "values", array_values_iter, 0);
    method(vm, array_proto, "entries", array_entries_iter, 0);

    // String
    let string_proto = vm.protos.string;
    let string_ctor = constructor(vm, "String", string_ctor, 1, string_proto);
    method(vm, string_ctor, "fromCharCode", string_from_char_code, 1);
    method(vm, string_proto, "toString", string_to_string, 0);
    method(vm, string_proto, "valueOf", string_to_string, 0);
    method(vm, string_proto, "slice", string_slice, 2);
    method(vm, string_proto, "substring", string_substring, 2);
    method(vm, string_proto, "indexOf", string_index_of, 1);
    method(vm, string_proto, "lastIndexOf", string_last_index_of, 1);
    method(vm, string_proto, "includes", string_includes, 1);
    method(vm, string_proto, "startsWith", string_starts_with, 1);
    method(vm, string_proto, "endsWith", string_ends_with, 1);
    method(vm, string_proto, "split", string_split, 2);
    method(vm, string_proto, "trim", string_trim, 0);
    method(vm, string_proto, "trimStart", string_trim_start, 0);
    method(vm, string_proto, "trimLeft", string_trim_start, 0);
    method(vm, string_proto, "trimEnd", string_trim_end, 0);
    method(vm, string_proto, "trimRight", string_trim_end, 0);
    method(vm, string_proto, "toLowerCase", string_to_lower, 0);
    method(vm, string_proto, "toUpperCase", string_to_upper, 0);
    method(vm, string_proto, "replace", string_replace, 2);
    method(vm, string_proto, "replaceAll", string_replace_all, 2);
    method(vm, string_proto, "repeat", string_repeat, 1);
    method(vm, string_proto, "padStart", string_pad_start, 1);
    method(vm, string_proto, "padEnd", string_pad_end, 1);
    method(vm, string_proto, "charAt", string_char_at, 1);
    method(vm, string_proto, "charCodeAt", string_char_code_at, 1);
    method(vm, string_proto, "concat", string_concat, 1);

    // Number
    let number_proto = vm.protos.number;
    let number_ctor = constructor(vm, "Number", number_ctor, 1, number_proto);
    define(
        vm,
        number_ctor,
        "MAX_SAFE_INTEGER",
        Value::number(9_007_199_254_740_991.0),
    );
    define(
        vm,
        number_ctor,
        "MIN_SAFE_INTEGER",
        Value::number(-9_007_199_254_740_991.0),
    );
    define(vm, number_ctor, "MAX_VALUE", Value::number(f64::MAX));
    define(
        vm,
        number_ctor,
        "MIN_VALUE",
        Value::number(f64::from_bits(1)),
    );
    define(vm, number_ctor, "EPSILON", Value::number(f64::EPSILON));
    define(vm, number_ctor, "NaN", Value::number(f64::NAN));
    define(
        vm,
        number_ctor,
        "POSITIVE_INFINITY",
        Value::number(f64::INFINITY),
    );
    define(
        vm,
        number_ctor,
        "NEGATIVE_INFINITY",
        Value::number(f64::NEG_INFINITY),
    );
    method(vm, number_ctor, "isNaN", number_is_nan, 1);
    method(vm, number_ctor, "isFinite", number_is_finite, 1);
    method(vm, number_ctor, "isInteger", number_is_integer, 1);
    method(vm, number_ctor, "isSafeInteger", number_is_safe_integer, 1);
    method(vm, number_ctor, "parseInt", parse_int, 2);
    method(vm, number_ctor, "parseFloat", parse_float, 1);
    method(vm, number_proto, "toString", number_to_string_method, 1);
    method(vm, number_proto, "toFixed", number_to_fixed, 1);
    method(vm, number_proto, "valueOf", number_value_of, 0);

    // Boolean
    let boolean_proto = vm.protos.boolean;
    constructor(vm, "Boolean", boolean_ctor, 1, boolean_proto);
    method(vm, boolean_proto, "toString", boolean_to_string, 0);
    method(vm, boolean_proto, "valueOf", boolean_value_of, 0);

    // Map & Set & Iterator
    let map_proto = vm.protos.map;
    constructor(vm, "Map", map_ctor, 0, map_proto);
    method(vm, map_proto, "get", map_get, 1);
    method(vm, map_proto, "set", map_set, 2);
    method(vm, map_proto, "has", map_has, 1);
    method(vm, map_proto, "delete", map_delete, 1);
    method(vm, map_proto, "clear", map_clear, 0);
    method(vm, map_proto, "forEach", map_for_each, 1);
    method(vm, map_proto, "keys", map_keys, 0);
    method(vm, map_proto, "values", map_values, 0);
    method(vm, map_proto, "entries", map_entries, 0);

    let set_proto = vm.protos.set;
    constructor(vm, "Set", set_ctor, 0, set_proto);
    method(vm, set_proto, "add", set_add, 1);
    method(vm, set_proto, "has", set_has, 1);
    method(vm, set_proto, "delete", set_delete, 1);
    method(vm, set_proto, "clear", set_clear, 0);
    method(vm, set_proto, "forEach", set_for_each, 1);
    method(vm, set_proto, "values", set_values, 0);
    method(vm, set_proto, "keys", set_values, 0);
    method(vm, set_proto, "entries", set_entries, 0);

    let iter_proto = vm.protos.iterator;
    method(vm, iter_proto, "next", iterator_next, 0);

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
        p.define_own(k_name, Value::string(n), PROP_METHOD);
        p.define_own(k_msg, Value::string(empty), PROP_METHOD);
    }

    // JSON
    let json_obj = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    define(vm, global, "JSON", Value::object(json_obj));
    method(vm, json_obj, "parse", json::parse, 2);
    method(vm, json_obj, "stringify", json::stringify, 3);

    // Math
    let math = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    define(vm, global, "Math", Value::object(math));
    define(vm, math, "E", Value::number(std::f64::consts::E));
    define(vm, math, "LN2", Value::number(std::f64::consts::LN_2));
    define(vm, math, "LN10", Value::number(std::f64::consts::LN_10));
    define(vm, math, "LOG2E", Value::number(std::f64::consts::LOG2_E));
    define(vm, math, "LOG10E", Value::number(std::f64::consts::LOG10_E));
    define(vm, math, "PI", Value::number(std::f64::consts::PI));
    define(
        vm,
        math,
        "SQRT1_2",
        Value::number(std::f64::consts::FRAC_1_SQRT_2),
    );
    define(vm, math, "SQRT2", Value::number(std::f64::consts::SQRT_2));

    method(vm, math, "abs", |vm, _, a| math1(vm, a, f64::abs), 1);
    method(vm, math, "acos", |vm, _, a| math1(vm, a, f64::acos), 1);
    method(vm, math, "acosh", |vm, _, a| math1(vm, a, f64::acosh), 1);
    method(vm, math, "asin", |vm, _, a| math1(vm, a, f64::asin), 1);
    method(vm, math, "asinh", |vm, _, a| math1(vm, a, f64::asinh), 1);
    method(vm, math, "atan", |vm, _, a| math1(vm, a, f64::atan), 1);
    method(vm, math, "atan2", math_atan2, 2);
    method(vm, math, "atanh", |vm, _, a| math1(vm, a, f64::atanh), 1);
    method(vm, math, "cbrt", |vm, _, a| math1(vm, a, f64::cbrt), 1);
    method(vm, math, "ceil", |vm, _, a| math1(vm, a, f64::ceil), 1);
    method(vm, math, "clz32", math_clz32, 1);
    method(vm, math, "cos", |vm, _, a| math1(vm, a, f64::cos), 1);
    method(vm, math, "cosh", |vm, _, a| math1(vm, a, f64::cosh), 1);
    method(vm, math, "exp", |vm, _, a| math1(vm, a, f64::exp), 1);
    method(vm, math, "expm1", |vm, _, a| math1(vm, a, f64::exp_m1), 1);
    method(vm, math, "floor", |vm, _, a| math1(vm, a, f64::floor), 1);
    method(
        vm,
        math,
        "fround",
        |vm, _, a| math1(vm, a, |x| (x as f32) as f64),
        1,
    );
    method(vm, math, "hypot", math_hypot, 2);
    method(vm, math, "imul", math_imul, 2);
    method(vm, math, "log", |vm, _, a| math1(vm, a, f64::ln), 1);
    method(vm, math, "log1p", |vm, _, a| math1(vm, a, f64::ln_1p), 1);
    method(vm, math, "log2", |vm, _, a| math1(vm, a, f64::log2), 1);
    method(vm, math, "log10", |vm, _, a| math1(vm, a, f64::log10), 1);
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
    method(vm, math, "pow", math_pow, 2);
    method(vm, math, "random", math_random, 0);
    method(vm, math, "round", |vm, _, a| math1(vm, a, js_round), 1);
    method(vm, math, "sign", |vm, _, a| math1(vm, a, js_sign), 1);
    method(vm, math, "sin", |vm, _, a| math1(vm, a, f64::sin), 1);
    method(vm, math, "sinh", |vm, _, a| math1(vm, a, f64::sinh), 1);
    method(vm, math, "sqrt", |vm, _, a| math1(vm, a, f64::sqrt), 1);
    method(vm, math, "tan", |vm, _, a| math1(vm, a, f64::tan), 1);
    method(vm, math, "tanh", |vm, _, a| math1(vm, a, f64::tanh), 1);
    method(vm, math, "trunc", |vm, _, a| math1(vm, a, f64::trunc), 1);
}

// ------------------------------------------------------------------ helpers

fn to_integer_or_inf(vm: &mut Vm, v: Value) -> Result<f64, Value> {
    let n = vm.to_number(v)?;
    if n.is_nan() || n == 0.0 {
        Ok(0.0)
    } else {
        Ok(n.trunc())
    }
}

fn clamp_rel_index(idx: f64, len: usize) -> usize {
    if idx < 0.0 {
        (len as f64 + idx).max(0.0) as usize
    } else {
        idx.min(len as f64) as usize
    }
}

fn new_array_val(vm: &mut Vm, items: Vec<Value>) -> Value {
    let o = vm
        .heap
        .alloc_object(Some(vm.protos.array), ObjectKind::Array(items));
    Value::object(o)
}

fn require_array_items(vm: &mut Vm, this: Value, method: &str) -> Result<(GcRef, Vec<Value>), Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error(format!("Array.prototype.{method} called on non-object")));
    };
    match &vm.heap.object(o).kind {
        ObjectKind::Array(items) => Ok((o, items.clone())),
        _ => Err(vm.type_error(format!(
            "Array.prototype.{method} em objeto genérico não suportado"
        ))),
    }
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
            ObjectKind::Function(_)
            | ObjectKind::Native { .. }
            | ObjectKind::HostFunction { .. } => "[object Function]",
            ObjectKind::Map(_) => "[object Map]",
            ObjectKind::Set(_) => "[object Set]",
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

fn own_prop_exists(vm: &Vm, o: GcRef, key: GcRef) -> bool {
    let obj = vm.heap.object(o);
    let mut has = obj.get_own(key).is_some();
    if let ObjectKind::Array(items) = &obj.kind {
        has |= key == vm.names.length
            || array_index(vm.heap.string(key)).is_some_and(|i| i < items.len());
    }
    has
}

fn object_has_own_prop(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    if this.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    let key = vm.intern_value(arg(args, 0))?;
    let Some(o) = this.as_object() else {
        return Ok(Value::FALSE);
    };
    Ok(Value::boolean(own_prop_exists(vm, o, key)))
}

fn object_has_own(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let target = arg(args, 0);
    if target.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    let key = vm.intern_value(arg(args, 1))?;
    match target.kind() {
        Kind::Object(o) => Ok(Value::boolean(own_prop_exists(vm, o, key))),
        Kind::String(s_ref) => {
            let len = vm.heap.string(s_ref).encode_utf16().count();
            let has = key == vm.names.length
                || array_index(vm.heap.string(key)).is_some_and(|i| i < len);
            Ok(Value::boolean(has))
        }
        _ => Ok(Value::FALSE),
    }
}

fn object_get_prototype_of(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let v = arg(args, 0);
    if v.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    match v.kind() {
        Kind::Object(o) => Ok(vm
            .heap
            .object(o)
            .proto
            .map(Value::object)
            .unwrap_or(Value::NULL)),
        Kind::String(_) => Ok(Value::object(vm.protos.string)),
        Kind::Number(_) => Ok(Value::object(vm.protos.number)),
        Kind::Bool(_) => Ok(Value::object(vm.protos.boolean)),
        _ => Ok(Value::NULL),
    }
}

fn object_set_prototype_of(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let obj_val = arg(args, 0);
    let proto_val = arg(args, 1);
    if obj_val.is_nullish() {
        return Err(vm.type_error("Object.setPrototypeOf called on null or undefined"));
    }
    let proto_opt = match proto_val.kind() {
        Kind::Object(p) => Some(p),
        Kind::Null => None,
        _ => return Err(vm.type_error("Object prototype may only be an Object or null")),
    };
    if let Some(o) = obj_val.as_object() {
        if vm.heap.object(o).frozen {
            return Err(vm.type_error("Object is not extensible"));
        }
        vm.heap.object_mut(o).proto = proto_opt;
    }
    Ok(obj_val)
}

fn collect_own_enumerable_entries(vm: &mut Vm, target: Value) -> Result<Vec<(GcRef, Value)>, Value> {
    if target.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    let mut out = Vec::new();
    match target.kind() {
        Kind::Object(o) => {
            let arr_items = match &vm.heap.object(o).kind {
                ObjectKind::Array(items) => Some(items.clone()),
                _ => None,
            };
            if let Some(items) = arr_items {
                for (i, v) in items.into_iter().enumerate() {
                    let k = vm.heap.intern(&i.to_string());
                    out.push((k, v));
                }
            }
            let props: Vec<(GcRef, Value)> = vm
                .heap
                .object(o)
                .props
                .iter()
                .filter(|p| p.flags & PROP_ENUMERABLE != 0)
                .map(|p| (p.key, p.value))
                .collect();
            out.extend(props);
        }
        Kind::String(s_ref) => {
            let chars: Vec<String> = vm
                .heap
                .string(s_ref)
                .encode_utf16()
                .map(|u| String::from_utf16_lossy(&[u]))
                .collect();
            for (i, ch) in chars.into_iter().enumerate() {
                let k = vm.heap.intern(&i.to_string());
                let sv = vm.new_string(&ch);
                out.push((k, sv));
            }
        }
        _ => {}
    }
    Ok(out)
}

fn object_keys(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let entries = collect_own_enumerable_entries(vm, arg(args, 0))?;
    let keys: Vec<Value> = entries.into_iter().map(|(k, _)| Value::string(k)).collect();
    Ok(new_array_val(vm, keys))
}

fn object_values(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let entries = collect_own_enumerable_entries(vm, arg(args, 0))?;
    let vals: Vec<Value> = entries.into_iter().map(|(_, v)| v).collect();
    Ok(new_array_val(vm, vals))
}

fn object_entries(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let entries = collect_own_enumerable_entries(vm, arg(args, 0))?;
    let pairs: Vec<Value> = entries
        .into_iter()
        .map(|(k, v)| new_array_val(vm, vec![Value::string(k), v]))
        .collect();
    Ok(new_array_val(vm, pairs))
}

fn object_from_entries(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let items = vm.collect_iterable(arg(args, 0))?;
    let obj = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    for item in items {
        if item.as_object().is_none() {
            return Err(vm.type_error("Iterator value is not an entry object"));
        }
        let k_0 = vm.heap.intern("0");
        let k_1 = vm.heap.intern("1");
        let k = vm.get_property(item, k_0)?;
        let v = vm.get_property(item, k_1)?;
        let key = vm.intern_value(k)?;
        vm.heap.object_mut(obj).set_own(key, v);
    }
    Ok(Value::object(obj))
}

fn object_assign(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let target = arg(args, 0);
    if target.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    for src in args.iter().skip(1) {
        if src.is_nullish() {
            continue;
        }
        let entries = collect_own_enumerable_entries(vm, *src)?;
        for (k, v) in entries {
            vm.set_property(target, k, v)?;
        }
    }
    Ok(target)
}

fn object_create(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let proto_val = arg(args, 0);
    let proto_opt = match proto_val.kind() {
        Kind::Object(o) => Some(o),
        Kind::Null => None,
        _ => return Err(vm.type_error("Object prototype may only be an Object or null")),
    };
    let obj = vm.heap.alloc_object(proto_opt, ObjectKind::Ordinary);
    let props_arg = arg(args, 1);
    if !props_arg.is_undefined() {
        if props_arg.is_nullish() {
            return Err(vm.type_error("Cannot convert undefined or null to object"));
        }
        let entries = collect_own_enumerable_entries(vm, props_arg)?;
        for (k, desc) in entries {
            apply_define_property(vm, obj, k, desc)?;
        }
    }
    Ok(Value::object(obj))
}

fn object_freeze(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let val = arg(args, 0);
    if let Some(o) = val.as_object() {
        let obj = vm.heap.object_mut(o);
        obj.frozen = true;
        for p in &mut obj.props {
            p.flags &= !(PROP_WRITABLE | PROP_CONFIGURABLE);
        }
    }
    Ok(val)
}

fn object_is_frozen(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let val = arg(args, 0);
    let frozen = match val.as_object() {
        Some(o) => vm.heap.object(o).frozen,
        None => true,
    };
    Ok(Value::boolean(frozen))
}

fn object_is(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.same_value(arg(args, 0), arg(args, 1))))
}

fn object_define_property(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let target = arg(args, 0);
    let Some(o) = target.as_object() else {
        return Err(vm.type_error("Object.defineProperty called on non-object"));
    };
    let key = vm.intern_value(arg(args, 1))?;
    let desc = arg(args, 2);
    apply_define_property(vm, o, key, desc)?;
    Ok(target)
}

fn apply_define_property(vm: &mut Vm, o: GcRef, key: GcRef, desc: Value) -> Result<(), Value> {
    if desc.as_object().is_none() {
        return Err(vm.type_error("Property description must be an object"));
    }
    let k_val = vm.heap.intern("value");
    let k_writable = vm.heap.intern("writable");
    let k_enum = vm.heap.intern("enumerable");
    let k_conf = vm.heap.intern("configurable");

    let has_val = own_prop_exists(vm, desc.as_object().unwrap(), k_val)
        || vm
            .lookup(desc.as_object().unwrap(), k_val)
            .is_some();
    let desc_obj = desc.as_object().unwrap();
    let get_opt_bool = |vm: &mut Vm, k: GcRef| -> Result<Option<bool>, Value> {
        if vm.lookup(desc_obj, k).is_some() {
            let v = vm.get_property(desc, k)?;
            Ok(Some(vm.to_boolean(v)))
        } else {
            Ok(None)
        }
    };

    let new_val = if has_val {
        Some(vm.get_property(desc, k_val)?)
    } else {
        None
    };
    let w_opt = get_opt_bool(vm, k_writable)?;
    let e_opt = get_opt_bool(vm, k_enum)?;
    let c_opt = get_opt_bool(vm, k_conf)?;

    let existing = vm.heap.object(o).get_own_prop(key).copied();
    match existing {
        Some(cur) => {
            let is_conf = (cur.flags & PROP_CONFIGURABLE) != 0;
            let is_writ = (cur.flags & PROP_WRITABLE) != 0;
            if !is_conf {
                if c_opt == Some(true) || (e_opt.is_some() && e_opt != Some((cur.flags & PROP_ENUMERABLE) != 0)) {
                    return Err(vm.type_error("Cannot redefine property"));
                }
                if !is_writ {
                    if w_opt == Some(true) {
                        return Err(vm.type_error("Cannot redefine property"));
                    }
                    if let Some(nv) = new_val
                        && !vm.same_value(nv, cur.value)
                    {
                        return Err(vm.type_error("Cannot redefine property"));
                    }
                }
            }
            let mut flags = cur.flags;
            if let Some(w) = w_opt {
                if w { flags |= PROP_WRITABLE; } else { flags &= !PROP_WRITABLE; }
            }
            if let Some(e) = e_opt {
                if e { flags |= PROP_ENUMERABLE; } else { flags &= !PROP_ENUMERABLE; }
            }
            if let Some(c) = c_opt {
                if c { flags |= PROP_CONFIGURABLE; } else { flags &= !PROP_CONFIGURABLE; }
            }
            let val = new_val.unwrap_or(cur.value);
            vm.heap.object_mut(o).define_own(key, val, flags);
        }
        None => {
            if vm.heap.object(o).frozen {
                return Err(vm.type_error("Cannot define property, object is not extensible"));
            }
            let mut flags = 0u8;
            if w_opt == Some(true) { flags |= PROP_WRITABLE; }
            if e_opt == Some(true) { flags |= PROP_ENUMERABLE; }
            if c_opt == Some(true) { flags |= PROP_CONFIGURABLE; }
            let val = new_val.unwrap_or(Value::UNDEFINED);
            vm.heap.object_mut(o).define_own(key, val, flags);
        }
    }
    Ok(())
}

fn object_get_own_property_names(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let target = arg(args, 0);
    if target.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    let mut keys = Vec::new();
    match target.kind() {
        Kind::Object(o) => {
            let len = match &vm.heap.object(o).kind {
                ObjectKind::Array(items) => Some(items.len()),
                _ => None,
            };
            if let Some(len) = len {
                for i in 0..len {
                    let s = vm.heap.alloc_string(&i.to_string());
                    keys.push(Value::string(s));
                }
                keys.push(Value::string(vm.names.length));
            }
            for p in &vm.heap.object(o).props {
                keys.push(Value::string(p.key));
            }
        }
        Kind::String(s_ref) => {
            let len = vm.heap.string(s_ref).encode_utf16().count();
            for i in 0..len {
                let s = vm.heap.alloc_string(&i.to_string());
                keys.push(Value::string(s));
            }
            keys.push(Value::string(vm.names.length));
        }
        _ => {}
    }
    Ok(new_array_val(vm, keys))
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
    Ok(new_array_val(vm, items))
}

fn array_is_array(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(arg(args, 0).as_object().is_some_and(|o| {
        matches!(vm.heap.object(o).kind, ObjectKind::Array(_))
    })))
}

fn array_of(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(new_array_val(vm, args.to_vec()))
}

fn array_from(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let src = arg(args, 0);
    if src.is_nullish() {
        return Err(vm.type_error("Cannot convert undefined or null to object"));
    }
    let map_fn = arg(args, 1);
    let has_map = !map_fn.is_undefined();
    if has_map && !vm.is_callable(map_fn) {
        return Err(vm.type_error("Array.from mapFn must be a function"));
    }
    let this_arg = arg(args, 2);

    let mut raw = match vm.collect_iterable(src) {
        Ok(v) => v,
        Err(_) => {
            // Array-like fallback: lê `length` e índices `0..length`
            let len_v = vm.get_property(src, vm.names.length)?;
            let len = to_integer_or_inf(vm, len_v)?.clamp(0.0, u32::MAX as f64) as usize;
            let mut out = Vec::with_capacity(len.min(10_000));
            for i in 0..len {
                out.push(vm.get_keyed(src, Value::number(i as f64))?);
            }
            out
        }
    };

    if has_map {
        for (i, item) in raw.iter_mut().enumerate() {
            *item = vm.call_function(map_fn, this_arg, &[*item, Value::number(i as f64)])?;
        }
    }
    Ok(new_array_val(vm, raw))
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

fn array_pop(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Array.prototype.pop called on non-object"));
    };
    match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(items) => Ok(items.pop().unwrap_or(Value::UNDEFINED)),
        _ => Err(vm.type_error("Array.prototype.pop em objeto genérico não suportado")),
    }
}

fn array_shift(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Array.prototype.shift called on non-object"));
    };
    match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(items) => {
            if items.is_empty() {
                Ok(Value::UNDEFINED)
            } else {
                Ok(items.remove(0))
            }
        }
        _ => Err(vm.type_error("Array.prototype.shift em objeto genérico não suportado")),
    }
}

fn array_unshift(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Array.prototype.unshift called on non-object"));
    };
    match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(items) => {
            items.splice(0..0, args.iter().copied());
            Ok(Value::number(items.len() as f64))
        }
        _ => Err(vm.type_error("Array.prototype.unshift em objeto genérico não suportado")),
    }
}

fn array_slice(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "slice")?;
    let len = items.len();
    let s = if args.is_empty() || args[0].is_undefined() {
        0
    } else {
        let n = to_integer_or_inf(vm, args[0])?;
        clamp_rel_index(n, len)
    };
    let e = if args.len() < 2 || args[1].is_undefined() {
        len
    } else {
        let n = to_integer_or_inf(vm, args[1])?;
        clamp_rel_index(n, len)
    };
    let out = if s >= e { Vec::new() } else { items[s..e].to_vec() };
    Ok(new_array_val(vm, out))
}

fn array_splice(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (o, items) = require_array_items(vm, this, "splice")?;
    let len = items.len();
    if args.is_empty() {
        return Ok(new_array_val(vm, Vec::new()));
    }
    let start_n = to_integer_or_inf(vm, args[0])?;
    let start = clamp_rel_index(start_n, len);
    let del_count = if args.len() < 2 {
        len - start
    } else {
        let dc = to_integer_or_inf(vm, args[1])?;
        if dc <= 0.0 {
            0
        } else {
            (dc as usize).min(len - start)
        }
    };
    let insert = if args.len() > 2 { &args[2..] } else { &[] };
    let removed = match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(vec) => {
            let rem: Vec<Value> = vec.drain(start..start + del_count).collect();
            vec.splice(start..start, insert.iter().copied());
            rem
        }
        _ => Vec::new(),
    };
    Ok(new_array_val(vm, removed))
}

fn array_concat(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    if this.is_nullish() {
        return Err(vm.type_error("Array.prototype.concat called on null or undefined"));
    }
    let mut out = match this.as_object().map(|o| &vm.heap.object(o).kind) {
        Some(ObjectKind::Array(items)) => items.clone(),
        _ => vec![this],
    };
    for a in args {
        if let Some(o) = a.as_object()
            && let ObjectKind::Array(items) = &vm.heap.object(o).kind
        {
            out.extend_from_slice(items);
        } else {
            out.push(*a);
        }
    }
    Ok(new_array_val(vm, out))
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

fn array_index_of(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "indexOf")?;
    let len = items.len();
    if len == 0 {
        return Ok(Value::number(-1.0));
    }
    let search = arg(args, 0);
    let from = if args.len() > 1 {
        let n = to_integer_or_inf(vm, args[1])?;
        if n >= len as f64 {
            return Ok(Value::number(-1.0));
        }
        if n < 0.0 {
            (len as f64 + n).max(0.0) as usize
        } else {
            n as usize
        }
    } else {
        0
    };
    for (i, &item) in items.iter().enumerate().skip(from) {
        if vm.strict_equals(item, search) {
            return Ok(Value::number(i as f64));
        }
    }
    Ok(Value::number(-1.0))
}

fn array_last_index_of(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "lastIndexOf")?;
    let len = items.len();
    if len == 0 {
        return Ok(Value::number(-1.0));
    }
    let search = arg(args, 0);
    let from = if args.len() > 1 {
        let n = to_integer_or_inf(vm, args[1])?;
        if n < 0.0 {
            let k = len as f64 + n;
            if k < 0.0 {
                return Ok(Value::number(-1.0));
            }
            k as usize
        } else {
            (n as usize).min(len - 1)
        }
    } else {
        len - 1
    };
    for i in (0..=from).rev() {
        if vm.strict_equals(items[i], search) {
            return Ok(Value::number(i as f64));
        }
    }
    Ok(Value::number(-1.0))
}

fn array_includes(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "includes")?;
    let len = items.len();
    if len == 0 {
        return Ok(Value::FALSE);
    }
    let search = arg(args, 0);
    let from = if args.len() > 1 {
        let n = to_integer_or_inf(vm, args[1])?;
        if n >= len as f64 {
            return Ok(Value::FALSE);
        }
        if n < 0.0 {
            (len as f64 + n).max(0.0) as usize
        } else {
            n as usize
        }
    } else {
        0
    };
    for &item in items.iter().skip(from) {
        if vm.same_value_zero(item, search) {
            return Ok(Value::TRUE);
        }
    }
    Ok(Value::FALSE)
}

fn array_reverse(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Array.prototype.reverse called on non-object"));
    };
    match &mut vm.heap.object_mut(o).kind {
        ObjectKind::Array(items) => items.reverse(),
        _ => return Err(vm.type_error("Array.prototype.reverse called on non-array")),
    }
    Ok(this)
}

fn array_fill(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (o, items) = require_array_items(vm, this, "fill")?;
    let len = items.len();
    let val = arg(args, 0);
    let start = if args.len() > 1 && !args[1].is_undefined() {
        clamp_rel_index(to_integer_or_inf(vm, args[1])?, len)
    } else {
        0
    };
    let end = if args.len() > 2 && !args[2].is_undefined() {
        clamp_rel_index(to_integer_or_inf(vm, args[2])?, len)
    } else {
        len
    };
    if let ObjectKind::Array(vec) = &mut vm.heap.object_mut(o).kind {
        for slot in vec.iter_mut().take(end).skip(start) {
            *slot = val;
        }
    }
    Ok(this)
}

fn array_flat(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "flat")?;
    let depth = if args.is_empty() || args[0].is_undefined() {
        1usize
    } else {
        let n = to_integer_or_inf(vm, args[0])?;
        if n <= 0.0 {
            0
        } else if n.is_infinite() {
            10_000
        } else {
            (n as usize).min(10_000)
        }
    };
    let mut out = Vec::new();
    flatten_into(vm, &items, depth, &mut out);
    Ok(new_array_val(vm, out))
}

fn flatten_into(vm: &Vm, items: &[Value], depth: usize, out: &mut Vec<Value>) {
    for &item in items {
        if depth > 0
            && let Some(o) = item.as_object()
            && let ObjectKind::Array(sub) = &vm.heap.object(o).kind
        {
            flatten_into(vm, sub, depth - 1, out);
        } else {
            out.push(item);
        }
    }
}

fn require_callback(vm: &mut Vm, cb: Value, method: &str) -> Result<(), Value> {
    if !vm.is_callable(cb) {
        return Err(vm.type_error(format!("{cb:?} is not a function in Array.prototype.{method}")));
    }
    Ok(())
}

fn array_for_each(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "forEach")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "forEach")?;
    let this_arg = arg(args, 1);
    for (i, item) in items.into_iter().enumerate() {
        vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
    }
    Ok(Value::UNDEFINED)
}

fn array_map(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "map")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "map")?;
    let this_arg = arg(args, 1);
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let mapped = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        out.push(mapped);
    }
    Ok(new_array_val(vm, out))
}

fn array_filter(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "filter")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "filter")?;
    let this_arg = arg(args, 1);
    let mut out = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        let keep = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        if vm.to_boolean(keep) {
            out.push(item);
        }
    }
    Ok(new_array_val(vm, out))
}

fn array_reduce(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "reduce")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "reduce")?;
    let (mut acc, start) = if args.len() >= 2 {
        (args[1], 0)
    } else if items.is_empty() {
        return Err(vm.type_error("Reduce of empty array with no initial value"));
    } else {
        (items[0], 1)
    };
    for (i, &item) in items.iter().enumerate().skip(start) {
        acc = vm.call_function(
            cb,
            Value::UNDEFINED,
            &[acc, item, Value::number(i as f64), this],
        )?;
    }
    Ok(acc)
}

fn array_reduce_right(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "reduceRight")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "reduceRight")?;
    if items.is_empty() && args.len() < 2 {
        return Err(vm.type_error("Reduce of empty array with no initial value"));
    }
    let (mut acc, end) = if args.len() >= 2 {
        (args[1], items.len())
    } else {
        (items[items.len() - 1], items.len() - 1)
    };
    for i in (0..end).rev() {
        acc = vm.call_function(
            cb,
            Value::UNDEFINED,
            &[acc, items[i], Value::number(i as f64), this],
        )?;
    }
    Ok(acc)
}

fn array_find(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "find")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "find")?;
    let this_arg = arg(args, 1);
    for (i, item) in items.into_iter().enumerate() {
        let hit = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        if vm.to_boolean(hit) {
            return Ok(item);
        }
    }
    Ok(Value::UNDEFINED)
}

fn array_find_index(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "findIndex")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "findIndex")?;
    let this_arg = arg(args, 1);
    for (i, item) in items.into_iter().enumerate() {
        let hit = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        if vm.to_boolean(hit) {
            return Ok(Value::number(i as f64));
        }
    }
    Ok(Value::number(-1.0))
}

fn array_some(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "some")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "some")?;
    let this_arg = arg(args, 1);
    for (i, item) in items.into_iter().enumerate() {
        let hit = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        if vm.to_boolean(hit) {
            return Ok(Value::TRUE);
        }
    }
    Ok(Value::FALSE)
}

fn array_every(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "every")?;
    let cb = arg(args, 0);
    require_callback(vm, cb, "every")?;
    let this_arg = arg(args, 1);
    for (i, item) in items.into_iter().enumerate() {
        let hit = vm.call_function(cb, this_arg, &[item, Value::number(i as f64), this])?;
        if !vm.to_boolean(hit) {
            return Ok(Value::FALSE);
        }
    }
    Ok(Value::TRUE)
}

fn array_sort(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let (o, items) = require_array_items(vm, this, "sort")?;
    let cmp = arg(args, 0);
    if !cmp.is_undefined() && !vm.is_callable(cmp) {
        return Err(vm.type_error("The comparison function must be either a function or undefined"));
    }
    let undef_count = items.iter().filter(|v| v.is_undefined()).count();
    let mut defined: Vec<Value> = items.into_iter().filter(|v| !v.is_undefined()).collect();

    if cmp.is_undefined() {
        let mut keyed: Vec<(String, Value)> = Vec::with_capacity(defined.len());
        for v in defined {
            let s = vm.to_str(v)?.to_string();
            keyed.push((s, v));
        }
        keyed.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
        defined = keyed.into_iter().map(|(_, v)| v).collect();
    } else {
        merge_sort_js(vm, &mut defined, cmp)?;
    }

    defined.resize(defined.len() + undef_count, Value::UNDEFINED);
    if let ObjectKind::Array(vec) = &mut vm.heap.object_mut(o).kind {
        *vec = defined;
    }
    Ok(this)
}

fn merge_sort_js(vm: &mut Vm, slice: &mut [Value], cmp: Value) -> Result<(), Value> {
    let len = slice.len();
    if len <= 1 {
        return Ok(());
    }
    let mid = len / 2;
    merge_sort_js(vm, &mut slice[..mid], cmp)?;
    merge_sort_js(vm, &mut slice[mid..], cmp)?;
    let left = slice[..mid].to_vec();
    let right = slice[mid..].to_vec();
    let (mut i, mut j, mut k) = (0, 0, 0);
    while i < left.len() && j < right.len() {
        let res = vm.call_function(cmp, Value::UNDEFINED, &[left[i], right[j]])?;
        let n = vm.to_number(res)?;
        let ord = if n < 0.0 {
            Ordering::Less
        } else if n > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        };
        if ord != Ordering::Greater {
            slice[k] = left[i];
            i += 1;
        } else {
            slice[k] = right[j];
            j += 1;
        }
        k += 1;
    }
    while i < left.len() {
        slice[k] = left[i];
        i += 1;
        k += 1;
    }
    while j < right.len() {
        slice[k] = right[j];
        j += 1;
        k += 1;
    }
    Ok(())
}

fn array_keys_iter(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "keys")?;
    let keys: Vec<Value> = (0..items.len()).map(|i| Value::number(i as f64)).collect();
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Values {
            items: keys,
            idx: 0,
        }),
    );
    Ok(Value::object(it))
}

fn array_values_iter(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let (o, _) = require_array_items(vm, this, "values")?;
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Array { arr: o, idx: 0 }),
    );
    Ok(Value::object(it))
}

fn array_entries_iter(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let (_, items) = require_array_items(vm, this, "entries")?;
    let pairs: Vec<Value> = items
        .into_iter()
        .enumerate()
        .map(|(i, v)| new_array_val(vm, vec![Value::number(i as f64), v]))
        .collect();
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Values {
            items: pairs,
            idx: 0,
        }),
    );
    Ok(Value::object(it))
}

// ------------------------------------------------------------------- String

fn require_string(vm: &mut Vm, this: Value, method: &str) -> Result<String, Value> {
    if this.is_nullish() {
        return Err(vm.type_error(format!(
            "String.prototype.{method} called on null or undefined"
        )));
    }
    Ok(vm.to_str(this)?.to_string())
}

fn string_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::string(vm.heap.intern("")));
    }
    let s = vm.to_str(args[0])?;
    Ok(vm.new_string(&s))
}

fn string_from_char_code(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let mut units = Vec::with_capacity(args.len());
    for &a in args {
        let n = vm.to_number(a)?;
        units.push(to_int32(n) as u16);
    }
    let s = String::from_utf16_lossy(&units);
    Ok(vm.new_string(&s))
}

fn string_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    match this.kind() {
        Kind::String(_) => Ok(this),
        _ => Err(vm.type_error("String.prototype.toString requires a string")),
    }
}

fn string_slice(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "slice")?;
    let units: Vec<u16> = s.encode_utf16().collect();
    let len = units.len();
    let start = if args.is_empty() || args[0].is_undefined() {
        0
    } else {
        clamp_rel_index(to_integer_or_inf(vm, args[0])?, len)
    };
    let end = if args.len() < 2 || args[1].is_undefined() {
        len
    } else {
        clamp_rel_index(to_integer_or_inf(vm, args[1])?, len)
    };
    if start >= end {
        return Ok(vm.new_string(""));
    }
    Ok(vm.new_string(&String::from_utf16_lossy(&units[start..end])))
}

fn string_substring(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "substring")?;
    let units: Vec<u16> = s.encode_utf16().collect();
    let len = units.len();
    let mut a = if args.is_empty() || args[0].is_undefined() {
        0
    } else {
        to_integer_or_inf(vm, args[0])?.clamp(0.0, len as f64) as usize
    };
    let mut b = if args.len() < 2 || args[1].is_undefined() {
        len
    } else {
        to_integer_or_inf(vm, args[1])?.clamp(0.0, len as f64) as usize
    };
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    Ok(vm.new_string(&String::from_utf16_lossy(&units[a..b])))
}

fn find_u16(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(hay.len()));
    }
    if from + needle.len() > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

fn rfind_u16(hay: &[u16], needle: &[u16], max_pos: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(max_pos.min(hay.len()));
    }
    if needle.len() > hay.len() {
        return None;
    }
    let limit = max_pos.min(hay.len() - needle.len());
    (0..=limit).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}

fn string_index_of(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "indexOf")?;
    let search = vm.to_str(arg(args, 0))?;
    let hay: Vec<u16> = s.encode_utf16().collect();
    let needle: Vec<u16> = search.encode_utf16().collect();
    let pos = if args.len() > 1 && !args[1].is_undefined() {
        to_integer_or_inf(vm, args[1])?.clamp(0.0, hay.len() as f64) as usize
    } else {
        0
    };
    Ok(Value::number(
        find_u16(&hay, &needle, pos).map(|i| i as f64).unwrap_or(-1.0),
    ))
}

fn string_last_index_of(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "lastIndexOf")?;
    let search = vm.to_str(arg(args, 0))?;
    let hay: Vec<u16> = s.encode_utf16().collect();
    let needle: Vec<u16> = search.encode_utf16().collect();
    let pos = if args.len() > 1 {
        let n = vm.to_number(args[1])?;
        if n.is_nan() {
            hay.len()
        } else {
            n.trunc().clamp(0.0, hay.len() as f64) as usize
        }
    } else {
        hay.len()
    };
    Ok(Value::number(
        rfind_u16(&hay, &needle, pos)
            .map(|i| i as f64)
            .unwrap_or(-1.0),
    ))
}

fn string_includes(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "includes")?;
    let search = vm.to_str(arg(args, 0))?;
    let hay: Vec<u16> = s.encode_utf16().collect();
    let needle: Vec<u16> = search.encode_utf16().collect();
    let pos = if args.len() > 1 && !args[1].is_undefined() {
        to_integer_or_inf(vm, args[1])?.clamp(0.0, hay.len() as f64) as usize
    } else {
        0
    };
    Ok(Value::boolean(find_u16(&hay, &needle, pos).is_some()))
}

fn string_starts_with(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "startsWith")?;
    let search = vm.to_str(arg(args, 0))?;
    let hay: Vec<u16> = s.encode_utf16().collect();
    let needle: Vec<u16> = search.encode_utf16().collect();
    let pos = if args.len() > 1 && !args[1].is_undefined() {
        to_integer_or_inf(vm, args[1])?.clamp(0.0, hay.len() as f64) as usize
    } else {
        0
    };
    let ok = pos + needle.len() <= hay.len() && &hay[pos..pos + needle.len()] == &needle[..];
    Ok(Value::boolean(ok))
}

fn string_ends_with(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "endsWith")?;
    let search = vm.to_str(arg(args, 0))?;
    let hay: Vec<u16> = s.encode_utf16().collect();
    let needle: Vec<u16> = search.encode_utf16().collect();
    let end = if args.len() > 1 && !args[1].is_undefined() {
        to_integer_or_inf(vm, args[1])?.clamp(0.0, hay.len() as f64) as usize
    } else {
        hay.len()
    };
    let ok = needle.len() <= end && &hay[end - needle.len()..end] == &needle[..];
    Ok(Value::boolean(ok))
}

fn string_split(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "split")?;
    let limit = if args.len() > 1 && !args[1].is_undefined() {
        (to_int32(vm.to_number(args[1])?) as u32) as usize
    } else {
        usize::MAX
    };
    if limit == 0 {
        return Ok(new_array_val(vm, Vec::new()));
    }
    let sep_arg = arg(args, 0);
    if sep_arg.is_undefined() {
        let v = vm.new_string(&s);
        return Ok(new_array_val(vm, vec![v]));
    }
    let sep = vm.to_str(sep_arg)?;
    let mut parts = Vec::new();
    if sep.is_empty() {
        for u in s.encode_utf16().take(limit) {
            let ch = String::from_utf16_lossy(&[u]);
            parts.push(vm.new_string(&ch));
        }
    } else {
        for p in s.split(&*sep).take(limit) {
            parts.push(vm.new_string(p));
        }
    }
    Ok(new_array_val(vm, parts))
}

fn is_js_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

fn string_trim(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "trim")?;
    Ok(vm.new_string(s.trim_matches(is_js_ws)))
}

fn string_trim_start(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "trimStart")?;
    Ok(vm.new_string(s.trim_start_matches(is_js_ws)))
}

fn string_trim_end(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "trimEnd")?;
    Ok(vm.new_string(s.trim_end_matches(is_js_ws)))
}

fn string_to_lower(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "toLowerCase")?;
    Ok(vm.new_string(&s.to_lowercase()))
}

fn string_to_upper(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "toUpperCase")?;
    Ok(vm.new_string(&s.to_uppercase()))
}

fn string_replace(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "replace")?;
    let search = vm.to_str(arg(args, 0))?;
    let rep_val = arg(args, 1);
    let Some(byte_idx) = s.find(&*search) else {
        return Ok(vm.new_string(&s));
    };
    let rep_str = if vm.is_callable(rep_val) {
        let u16_pos = s[..byte_idx].encode_utf16().count();
        let matched = vm.new_string(&search);
        let full = vm.new_string(&s);
        let res = vm.call_function(
            rep_val,
            Value::UNDEFINED,
            &[matched, Value::number(u16_pos as f64), full],
        )?;
        vm.to_str(res)?.to_string()
    } else {
        vm.to_str(rep_val)?.to_string()
    };
    let mut out = String::with_capacity(s.len() - search.len() + rep_str.len());
    out.push_str(&s[..byte_idx]);
    out.push_str(&rep_str);
    out.push_str(&s[byte_idx + search.len()..]);
    Ok(vm.new_string(&out))
}

fn string_replace_all(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "replaceAll")?;
    let search = vm.to_str(arg(args, 0))?;
    let rep_val = arg(args, 1);
    let is_fn = vm.is_callable(rep_val);
    let static_rep = if is_fn {
        String::new()
    } else {
        vm.to_str(rep_val)?.to_string()
    };
    let full_val = if is_fn {
        vm.new_string(&s)
    } else {
        Value::UNDEFINED
    };

    if search.is_empty() {
        let units: Vec<u16> = s.encode_utf16().collect();
        let empty = vm.new_string("");
        let mut out = String::new();
        for i in 0..=units.len() {
            let piece = if is_fn {
                let res = vm.call_function(
                    rep_val,
                    Value::UNDEFINED,
                    &[empty, Value::number(i as f64), full_val],
                )?;
                vm.to_str(res)?.to_string()
            } else {
                static_rep.clone()
            };
            out.push_str(&piece);
            if i < units.len() {
                out.push_str(&String::from_utf16_lossy(&[units[i]]));
            }
        }
        return Ok(vm.new_string(&out));
    }

    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (idx, matched) in s.match_indices(&*search) {
        out.push_str(&s[last..idx]);
        if is_fn {
            let u16_pos = s[..idx].encode_utf16().count();
            let m_val = vm.new_string(matched);
            let res = vm.call_function(
                rep_val,
                Value::UNDEFINED,
                &[m_val, Value::number(u16_pos as f64), full_val],
            )?;
            out.push_str(&vm.to_str(res)?);
        } else {
            out.push_str(&static_rep);
        }
        last = idx + matched.len();
    }
    out.push_str(&s[last..]);
    Ok(vm.new_string(&out))
}

fn string_repeat(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "repeat")?;
    let n = to_integer_or_inf(vm, arg(args, 0))?;
    if n < 0.0 || n.is_infinite() {
        return Err(vm.range_error("Invalid count value"));
    }
    let count = n as usize;
    if count == 0 || s.is_empty() {
        return Ok(vm.new_string(""));
    }
    if s.len().saturating_mul(count) > 64 * 1024 * 1024 {
        return Err(vm.range_error("Invalid string length"));
    }
    Ok(vm.new_string(&s.repeat(count)))
}

fn pad_string(
    vm: &mut Vm,
    this: Value,
    args: &[Value],
    at_start: bool,
) -> Result<Value, Value> {
    let method = if at_start { "padStart" } else { "padEnd" };
    let s = require_string(vm, this, method)?;
    let target_len = to_integer_or_inf(vm, arg(args, 0))?.max(0.0) as usize;
    let s_units_len = s.encode_utf16().count();
    if target_len <= s_units_len {
        return Ok(vm.new_string(&s));
    }
    let filler = if args.len() > 1 && !args[1].is_undefined() {
        vm.to_str(args[1])?.to_string()
    } else {
        " ".to_owned()
    };
    if filler.is_empty() {
        return Ok(vm.new_string(&s));
    }
    let fill_len = target_len - s_units_len;
    let filler_units: Vec<u16> = filler.encode_utf16().collect();
    let mut pad_units = Vec::with_capacity(fill_len);
    for i in 0..fill_len {
        pad_units.push(filler_units[i % filler_units.len()]);
    }
    let pad = String::from_utf16_lossy(&pad_units);
    let out = if at_start {
        format!("{pad}{s}")
    } else {
        format!("{s}{pad}")
    };
    Ok(vm.new_string(&out))
}

fn string_pad_start(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    pad_string(vm, this, args, true)
}

fn string_pad_end(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    pad_string(vm, this, args, false)
}

fn string_char_at(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "charAt")?;
    let pos = if args.is_empty() || args[0].is_undefined() {
        0.0
    } else {
        to_integer_or_inf(vm, args[0])?
    };
    if pos < 0.0 || pos.is_infinite() {
        return Ok(vm.new_string(""));
    }
    let idx = pos as usize;
    match s.encode_utf16().nth(idx) {
        Some(u) => Ok(vm.new_string(&String::from_utf16_lossy(&[u]))),
        None => Ok(vm.new_string("")),
    }
}

fn string_char_code_at(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let s = require_string(vm, this, "charCodeAt")?;
    let pos = if args.is_empty() || args[0].is_undefined() {
        0.0
    } else {
        to_integer_or_inf(vm, args[0])?
    };
    if pos < 0.0 || pos.is_infinite() {
        return Ok(Value::number(f64::NAN));
    }
    let idx = pos as usize;
    match s.encode_utf16().nth(idx) {
        Some(u) => Ok(Value::number(u as f64)),
        None => Ok(Value::number(f64::NAN)),
    }
}

fn string_concat(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let mut out = require_string(vm, this, "concat")?;
    for &a in args {
        out.push_str(&vm.to_str(a)?);
    }
    Ok(vm.new_string(&out))
}

// ------------------------------------------------------------------- Number

fn number_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    if args.is_empty() {
        return Ok(Value::number(0.0));
    }
    Ok(Value::number(vm.to_number(args[0])?))
}

fn number_is_nan(_: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(matches!(
        arg(args, 0).kind(),
        Kind::Number(n) if n.is_nan()
    )))
}

fn number_is_finite(_: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(matches!(
        arg(args, 0).kind(),
        Kind::Number(n) if n.is_finite()
    )))
}

fn number_is_integer(_: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(matches!(
        arg(args, 0).kind(),
        Kind::Number(n) if n.is_finite() && n.fract() == 0.0
    )))
}

fn number_is_safe_integer(_: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(matches!(
        arg(args, 0).kind(),
        Kind::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0
    )))
}

fn number_value_of(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    match this.as_number() {
        Some(_) => Ok(this),
        None => Err(vm.type_error("Number.prototype.valueOf requires a number")),
    }
}

fn number_to_fixed(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(n) = this.as_number() else {
        return Err(vm.type_error("Number.prototype.toFixed requires a number"));
    };
    let d = if args.is_empty() || args[0].is_undefined() {
        0.0
    } else {
        to_integer_or_inf(vm, args[0])?
    };
    if !(0.0..=100.0).contains(&d) {
        return Err(vm.range_error("toFixed() digits argument must be between 0 and 100"));
    }
    let digits = d as usize;
    if !n.is_finite() || n.abs() >= 1e21 {
        return Ok(vm.new_string(&number_to_string(n)));
    }
    let n = if n == 0.0 { 0.0 } else { n };
    Ok(vm.new_string(&format!("{n:.digits$}")))
}

fn number_to_string_method(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(n) = this.as_number() else {
        return Err(vm.type_error("Number.prototype.toString requires a number"));
    };
    let radix = if args.is_empty() || args[0].is_undefined() {
        10
    } else {
        let r = to_integer_or_inf(vm, args[0])?;
        if !(2.0..=36.0).contains(&r) {
            return Err(vm.range_error("toString() radix argument must be between 2 and 36"));
        }
        r as u32
    };
    if radix == 10 || !n.is_finite() || n == 0.0 {
        return Ok(vm.new_string(&number_to_string(n)));
    }
    Ok(vm.new_string(&format_radix(n, radix)))
}

fn format_radix(n: f64, radix: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let neg = n < 0.0;
    let abs = n.abs();
    let r = radix as f64;

    let mut int_part = abs.trunc();
    let mut frac = abs - int_part;
    let mut int_chars = Vec::new();
    if int_part == 0.0 {
        int_chars.push(b'0');
    } else {
        while int_part >= 1.0 {
            let rem = (int_part % r) as usize;
            int_chars.push(DIGITS[rem.min(radix as usize - 1)]);
            int_part = (int_part / r).floor();
        }
        int_chars.reverse();
    }

    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&int_chars).unwrap_or("0"));
    if frac > 0.0 {
        out.push('.');
        for _ in 0..52 {
            frac *= r;
            let d = (frac.trunc() as usize).min(radix as usize - 1);
            out.push(DIGITS[d] as char);
            frac -= d as f64;
            if frac <= f64::EPSILON {
                break;
            }
        }
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    out
}

fn boolean_ctor(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    Ok(Value::boolean(vm.to_boolean(arg(args, 0))))
}

fn boolean_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    match this.kind() {
        Kind::Bool(b) => Ok(vm.new_string(if b { "true" } else { "false" })),
        _ => Err(vm.type_error("Boolean.prototype.toString requires a boolean")),
    }
}

fn boolean_value_of(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    match this.kind() {
        Kind::Bool(_) => Ok(this),
        _ => Err(vm.type_error("Boolean.prototype.valueOf requires a boolean")),
    }
}

// ------------------------------------------------------------------ Map/Set

fn map_ctor(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(obj) = this.as_object() else {
        return Err(vm.type_error("Constructor Map requires 'new'"));
    };
    if obj == vm.global {
        return Err(vm.type_error("Constructor Map requires 'new'"));
    }
    vm.heap.object_mut(obj).kind = ObjectKind::Map(JsMap::default());
    let init = arg(args, 0);
    if !init.is_nullish() {
        let items = vm.collect_iterable(init)?;
        for item in items {
            if item.as_object().is_none() {
                return Err(vm.type_error("Iterator value is not an entry object"));
            }
            let k0 = vm.heap.intern("0");
            let k1 = vm.heap.intern("1");
            let k = vm.get_property(item, k0)?;
            let v = vm.get_property(item, k1)?;
            let mk = MapKey::from_value(k, &vm.heap);
            if let ObjectKind::Map(m) = &mut vm.heap.object_mut(obj).kind {
                m.set(mk, k, v);
            }
        }
    }
    Ok(Value::object(obj))
}

fn require_map(vm: &mut Vm, this: Value, method: &str) -> Result<GcRef, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error(format!("Method Map.prototype.{method} called on non-object")));
    };
    if !matches!(vm.heap.object(o).kind, ObjectKind::Map(_)) {
        return Err(vm.type_error(format!("Method Map.prototype.{method} called on incompatible receiver")));
    }
    Ok(o)
}

fn map_get(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "get")?;
    let key = MapKey::from_value(arg(args, 0), &vm.heap);
    let ObjectKind::Map(m) = &vm.heap.object(o).kind else {
        unreachable!()
    };
    Ok(m.get(&key).unwrap_or(Value::UNDEFINED))
}

fn map_set(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "set")?;
    let k_val = arg(args, 0);
    let v_val = arg(args, 1);
    let key = MapKey::from_value(k_val, &vm.heap);
    if let ObjectKind::Map(m) = &mut vm.heap.object_mut(o).kind {
        m.set(key, k_val, v_val);
    }
    Ok(this)
}

fn map_has(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "has")?;
    let key = MapKey::from_value(arg(args, 0), &vm.heap);
    let ObjectKind::Map(m) = &vm.heap.object(o).kind else {
        unreachable!()
    };
    Ok(Value::boolean(m.has(&key)))
}

fn map_delete(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "delete")?;
    let key = MapKey::from_value(arg(args, 0), &vm.heap);
    let ObjectKind::Map(m) = &mut vm.heap.object_mut(o).kind else {
        unreachable!()
    };
    Ok(Value::boolean(m.delete(&key)))
}

fn map_clear(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "clear")?;
    if let ObjectKind::Map(m) = &mut vm.heap.object_mut(o).kind {
        m.clear();
    }
    Ok(Value::UNDEFINED)
}

fn map_for_each(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_map(vm, this, "forEach")?;
    let cb = arg(args, 0);
    if !vm.is_callable(cb) {
        return Err(vm.type_error("Map.prototype.forEach callback is not a function"));
    }
    let this_arg = arg(args, 1);
    let pairs: Vec<(Value, Value)> = match &vm.heap.object(o).kind {
        ObjectKind::Map(m) => m.entries.iter().flatten().copied().collect(),
        _ => Vec::new(),
    };
    for (k, v) in pairs {
        vm.call_function(cb, this_arg, &[v, k, this])?;
    }
    Ok(Value::UNDEFINED)
}

fn map_iter(vm: &mut Vm, this: Value, mode: MapIterMode, name: &str) -> Result<Value, Value> {
    let o = require_map(vm, this, name)?;
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Map {
            map: o,
            idx: 0,
            mode,
        }),
    );
    Ok(Value::object(it))
}

fn map_keys(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    map_iter(vm, this, MapIterMode::Keys, "keys")
}

fn map_values(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    map_iter(vm, this, MapIterMode::Values, "values")
}

fn map_entries(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    map_iter(vm, this, MapIterMode::Entries, "entries")
}

fn set_ctor(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let Some(obj) = this.as_object() else {
        return Err(vm.type_error("Constructor Set requires 'new'"));
    };
    if obj == vm.global {
        return Err(vm.type_error("Constructor Set requires 'new'"));
    }
    vm.heap.object_mut(obj).kind = ObjectKind::Set(JsSet::default());
    let init = arg(args, 0);
    if !init.is_nullish() {
        let items = vm.collect_iterable(init)?;
        for v in items {
            let sk = MapKey::from_value(v, &vm.heap);
            if let ObjectKind::Set(s) = &mut vm.heap.object_mut(obj).kind {
                s.add(sk, v);
            }
        }
    }
    Ok(Value::object(obj))
}

fn require_set(vm: &mut Vm, this: Value, method: &str) -> Result<GcRef, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error(format!("Method Set.prototype.{method} called on non-object")));
    };
    if !matches!(vm.heap.object(o).kind, ObjectKind::Set(_)) {
        return Err(vm.type_error(format!("Method Set.prototype.{method} called on incompatible receiver")));
    }
    Ok(o)
}

fn set_add(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "add")?;
    let v = arg(args, 0);
    let key = MapKey::from_value(v, &vm.heap);
    if let ObjectKind::Set(s) = &mut vm.heap.object_mut(o).kind {
        s.add(key, v);
    }
    Ok(this)
}

fn set_has(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "has")?;
    let key = MapKey::from_value(arg(args, 0), &vm.heap);
    let ObjectKind::Set(s) = &vm.heap.object(o).kind else {
        unreachable!()
    };
    Ok(Value::boolean(s.has(&key)))
}

fn set_delete(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "delete")?;
    let key = MapKey::from_value(arg(args, 0), &vm.heap);
    let ObjectKind::Set(s) = &mut vm.heap.object_mut(o).kind else {
        unreachable!()
    };
    Ok(Value::boolean(s.delete(&key)))
}

fn set_clear(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "clear")?;
    if let ObjectKind::Set(s) = &mut vm.heap.object_mut(o).kind {
        s.clear();
    }
    Ok(Value::UNDEFINED)
}

fn set_for_each(vm: &mut Vm, this: Value, args: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "forEach")?;
    let cb = arg(args, 0);
    if !vm.is_callable(cb) {
        return Err(vm.type_error("Set.prototype.forEach callback is not a function"));
    }
    let this_arg = arg(args, 1);
    let vals: Vec<Value> = match &vm.heap.object(o).kind {
        ObjectKind::Set(s) => s.entries.iter().flatten().copied().collect(),
        _ => Vec::new(),
    };
    for v in vals {
        vm.call_function(cb, this_arg, &[v, v, this])?;
    }
    Ok(Value::UNDEFINED)
}

fn set_values(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "values")?;
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Set {
            set: o,
            idx: 0,
            entries_mode: false,
        }),
    );
    Ok(Value::object(it))
}

fn set_entries(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let o = require_set(vm, this, "entries")?;
    let it = vm.heap.alloc_object(
        Some(vm.protos.iterator),
        ObjectKind::Iterator(IterKind::Set {
            set: o,
            idx: 0,
            entries_mode: true,
        }),
    );
    Ok(Value::object(it))
}

fn iterator_next(vm: &mut Vm, this: Value, _: &[Value]) -> Result<Value, Value> {
    let Some(o) = this.as_object() else {
        return Err(vm.type_error("Iterator.prototype.next called on non-object"));
    };
    if !matches!(vm.heap.object(o).kind, ObjectKind::Iterator(_)) {
        return Err(vm.type_error("Iterator.prototype.next called on non-iterator"));
    }
    let res = vm.iter_next(o)?;
    let obj = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    let (k_val, k_done) = (vm.names.value, vm.names.done);
    let (val, done) = match res {
        Some(v) => (v, Value::FALSE),
        None => (Value::UNDEFINED, Value::TRUE),
    };
    let o_mut = vm.heap.object_mut(obj);
    o_mut.set_own(k_val, val);
    o_mut.set_own(k_done, done);
    Ok(Value::object(obj))
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

fn math_atan2(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let y = vm.to_number(arg(args, 0))?;
    let x = vm.to_number(arg(args, 1))?;
    Ok(Value::number(y.atan2(x)))
}

fn math_clz32(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let x = to_int32(vm.to_number(arg(args, 0))?) as u32;
    Ok(Value::number(x.leading_zeros() as f64))
}

fn math_imul(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let a = to_int32(vm.to_number(arg(args, 0))?);
    let b = to_int32(vm.to_number(arg(args, 1))?);
    Ok(Value::number(a.wrapping_mul(b) as f64))
}

fn math_hypot(vm: &mut Vm, _: Value, args: &[Value]) -> Result<Value, Value> {
    let mut nums = Vec::with_capacity(args.len());
    let mut has_inf = false;
    let mut has_nan = false;
    for &a in args {
        let n = vm.to_number(a)?;
        if n.is_infinite() {
            has_inf = true;
        } else if n.is_nan() {
            has_nan = true;
        }
        nums.push(n.abs());
    }
    if has_inf {
        return Ok(Value::number(f64::INFINITY));
    }
    if has_nan {
        return Ok(Value::number(f64::NAN));
    }
    let max = nums.iter().copied().fold(0.0f64, f64::max);
    if max == 0.0 {
        return Ok(Value::number(0.0));
    }
    let sum_sq: f64 = nums.into_iter().map(|v| (v / max).powi(2)).sum();
    Ok(Value::number(max * sum_sq.sqrt()))
}

fn js_sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else {
        x.signum()
    }
}

fn js_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 || x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 {
        let r = f + 1.0;
        if r == 0.0 && x < 0.0 { -0.0 } else { r }
    } else {
        f
    }
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
