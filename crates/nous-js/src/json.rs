//! Implementação pura em Rust de `JSON.parse` e `JSON.stringify`.

use crate::heap::{GcRef, ObjectKind, PROP_ENUMERABLE};
use crate::value::{Kind, Value};
use crate::vm::{Vm, number_to_string};

const MAX_DEPTH: usize = 512;

pub fn parse(vm: &mut Vm, _this: Value, args: &[Value]) -> Result<Value, Value> {
    let input = args.first().copied().unwrap_or(Value::UNDEFINED);
    let src = vm.to_str(input)?;
    let mut parser = JsonParser {
        vm,
        bytes: src.as_bytes(),
        pos: 0,
        depth: 0,
    };
    parser.skip_ws();
    let val = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos < parser.bytes.len() {
        return Err(parser.syntax_error("Unexpected non-whitespace character after JSON"));
    }
    let reviver = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    if vm.is_callable(reviver) {
        let holder = vm
            .heap
            .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
        let empty = vm.heap.intern("");
        vm.heap.object_mut(holder).set_own(empty, val);
        return internalize(vm, reviver, Value::object(holder), empty);
    }
    Ok(val)
}

fn internalize(
    vm: &mut Vm,
    reviver: Value,
    holder: Value,
    key: GcRef,
) -> Result<Value, Value> {
    let val = vm.get_property(holder, key)?;
    if let Some(o) = val.as_object() {
        enum Kids {
            Arr(usize),
            Obj(Vec<GcRef>),
        }
        let kids = match &vm.heap.object(o).kind {
            ObjectKind::Array(items) => Kids::Arr(items.len()),
            ObjectKind::Function(_)
            | ObjectKind::Native { .. }
            | ObjectKind::HostFunction { .. } => Kids::Obj(Vec::new()),
            _ => Kids::Obj(
                vm.heap
                    .object(o)
                    .props
                    .iter()
                    .filter(|p| p.flags & PROP_ENUMERABLE != 0)
                    .map(|p| p.key)
                    .collect(),
            ),
        };
        match kids {
            Kids::Arr(len) => {
                for i in 0..len {
                    let k = vm.heap.intern(&i.to_string());
                    let new_el = internalize(vm, reviver, val, k)?;
                    if new_el.is_undefined() {
                        if let ObjectKind::Array(items) = &mut vm.heap.object_mut(o).kind
                            && i < items.len()
                        {
                            items[i] = Value::UNDEFINED;
                        }
                    } else {
                        vm.set_property(val, k, new_el)?;
                    }
                }
            }
            Kids::Obj(keys) => {
                for k in keys {
                    let new_el = internalize(vm, reviver, val, k)?;
                    if new_el.is_undefined() {
                        vm.heap.object_mut(o).delete_own(k);
                    } else {
                        vm.set_property(val, k, new_el)?;
                    }
                }
            }
        }
    }
    vm.call_function(reviver, holder, &[Value::string(key), val])
}

struct JsonParser<'a> {
    vm: &'a mut Vm,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl JsonParser<'_> {
    fn syntax_error(&mut self, msg: &str) -> Value {
        self.vm.make_error(self.vm.protos.syntax_error, msg)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<Value, Value> {
        self.skip_ws();
        let Some(b) = self.peek() else {
            return Err(self.syntax_error("Unexpected end of JSON input"));
        };
        match b {
            b'n' => self.parse_lit(b"null", Value::NULL),
            b't' => self.parse_lit(b"true", Value::TRUE),
            b'f' => self.parse_lit(b"false", Value::FALSE),
            b'"' => {
                let s = self.parse_string()?;
                Ok(self.vm.new_string(&s))
            }
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(self.syntax_error("Unexpected token in JSON")),
        }
    }

    fn parse_lit(&mut self, expected: &[u8], val: Value) -> Result<Value, Value> {
        if self.bytes[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(val)
        } else {
            Err(self.syntax_error("Unexpected token in JSON"))
        }
    }

    fn parse_array(&mut self) -> Result<Value, Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.syntax_error("JSON nesting too deep"));
        }
        self.pos += 1; // '['
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            let o = self
                .vm
                .heap
                .alloc_object(Some(self.vm.protos.array), ObjectKind::Array(items));
            return Ok(Value::object(o));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.syntax_error("Expected ',' or ']' in JSON array")),
            }
        }
        self.depth -= 1;
        let o = self
            .vm
            .heap
            .alloc_object(Some(self.vm.protos.array), ObjectKind::Array(items));
        Ok(Value::object(o))
    }

    fn parse_object(&mut self) -> Result<Value, Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.syntax_error("JSON nesting too deep"));
        }
        self.pos += 1; // '{'
        self.skip_ws();
        let obj = self
            .vm
            .heap
            .alloc_object(Some(self.vm.protos.object), ObjectKind::Ordinary);
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::object(obj));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.syntax_error("Expected string property name in JSON object"));
            }
            let key_str = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.syntax_error("Expected ':' after property name in JSON object"));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            let key = self.vm.heap.intern(&key_str);
            self.vm.heap.object_mut(obj).set_own(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.syntax_error("Expected ',' or '}' in JSON object")),
            }
        }
        self.depth -= 1;
        Ok(Value::object(obj))
    }

    fn parse_string(&mut self) -> Result<String, Value> {
        self.pos += 1; // '"'
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(out);
            }
            if b < 0x20 {
                return Err(self.syntax_error("Unescaped control character in JSON string"));
            }
            if b == b'\\' {
                self.pos += 1;
                let Some(esc) = self.peek() else {
                    return Err(self.syntax_error("Unterminated escape sequence in JSON string"));
                };
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let u1 = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&u1)
                            && self.bytes[self.pos..].starts_with(b"\\u")
                        {
                            let saved = self.pos;
                            self.pos += 2;
                            if let Ok(u2) = self.parse_hex4()
                                && (0xDC00..=0xDFFF).contains(&u2)
                            {
                                let cp = 0x10000
                                    + (((u1 as u32 - 0xD800) << 10) | (u2 as u32 - 0xDC00));
                                if let Some(ch) = char::from_u32(cp) {
                                    out.push(ch);
                                    continue;
                                }
                            }
                            self.pos = saved;
                        }
                        let ch = char::from_u32(u1 as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
                        out.push(ch);
                    }
                    _ => return Err(self.syntax_error("Invalid escape sequence in JSON string")),
                }
                continue;
            }
            // UTF-8 válido no trecho contínuo sem escapes/aspas
            let rest = &self.bytes[self.pos..];
            let s = std::str::from_utf8(rest)
                .map_err(|_| self.syntax_error("Invalid UTF-8 in JSON string"))?;
            let ch = s.chars().next().expect("não vazio");
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err(self.syntax_error("Unterminated string in JSON"))
    }

    fn parse_hex4(&mut self) -> Result<u16, Value> {
        if self.pos + 4 > self.bytes.len() {
            return Err(self.syntax_error("Invalid Unicode escape in JSON"));
        }
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| self.syntax_error("Invalid Unicode escape in JSON"))?;
        let val = u16::from_str_radix(s, 16)
            .map_err(|_| self.syntax_error("Invalid Unicode escape in JSON"))?;
        self.pos += 4;
        Ok(val)
    }

    fn parse_number(&mut self) -> Result<Value, Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.syntax_error("Unexpected leading zero in JSON number"));
                }
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.syntax_error("Invalid number in JSON")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.syntax_error("Expected digit after decimal point in JSON"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.syntax_error("Expected digit in JSON exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let slice = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.syntax_error("Invalid number in JSON"))?;
        let n = slice
            .parse::<f64>()
            .map_err(|_| self.syntax_error("Invalid number in JSON"))?;
        Ok(Value::number(n))
    }
}

// ----------------------------------------------------------------- stringify

enum Replacer {
    None,
    Fn(Value),
    List(Vec<GcRef>),
}

pub fn stringify(vm: &mut Vm, _this: Value, args: &[Value]) -> Result<Value, Value> {
    let val = args.first().copied().unwrap_or(Value::UNDEFINED);
    let rep_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    let space_arg = args.get(2).copied().unwrap_or(Value::UNDEFINED);

    let replacer = if vm.is_callable(rep_arg) {
        Replacer::Fn(rep_arg)
    } else if let Some(o) = rep_arg.as_object()
        && let ObjectKind::Array(items) = &vm.heap.object(o).kind
    {
        let items = items.clone();
        let mut keys = Vec::new();
        for item in items {
            if item.as_string().is_some() || item.is_number() {
                let k = vm.intern_value(item)?;
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        Replacer::List(keys)
    } else {
        Replacer::None
    };

    let gap = match space_arg.kind() {
        Kind::Number(n) => {
            let count = if n.is_nan() {
                0
            } else {
                (n.trunc().clamp(0.0, 10.0)) as usize
            };
            " ".repeat(count)
        }
        Kind::String(r) => {
            let s = vm.heap.string(r);
            s.chars().take(10).collect()
        }
        _ => String::new(),
    };

    let wrapper = vm
        .heap
        .alloc_object(Some(vm.protos.object), ObjectKind::Ordinary);
    let empty_key = vm.heap.intern("");
    vm.heap.object_mut(wrapper).set_own(empty_key, val);

    let mut state = StringifyState {
        vm,
        replacer,
        gap,
        indent: String::new(),
        stack: Vec::new(),
    };
    match state.serialize_prop(Value::object(wrapper), empty_key)? {
        Some(s) => Ok(state.vm.new_string(&s)),
        None => Ok(Value::UNDEFINED),
    }
}

struct StringifyState<'a> {
    vm: &'a mut Vm,
    replacer: Replacer,
    gap: String,
    indent: String,
    stack: Vec<GcRef>,
}

impl StringifyState<'_> {
    fn serialize_prop(&mut self, holder: Value, key: GcRef) -> Result<Option<String>, Value> {
        let mut val = self.vm.get_property(holder, key)?;
        if val.as_object().is_some() {
            let to_json_key = self.vm.heap.intern("toJSON");
            let to_json = self.vm.get_property(val, to_json_key)?;
            if self.vm.is_callable(to_json) {
                val = self.vm.call_function(to_json, val, &[Value::string(key)])?;
            }
        }
        if let Replacer::Fn(f) = self.replacer {
            val = self
                .vm
                .call_function(f, holder, &[Value::string(key), val])?;
        }
        if self.vm.is_callable(val) || val.is_undefined() {
            return Ok(None);
        }
        match val.kind() {
            Kind::Null => Ok(Some("null".into())),
            Kind::Bool(true) => Ok(Some("true".into())),
            Kind::Bool(false) => Ok(Some("false".into())),
            Kind::Number(n) => {
                if !n.is_finite() {
                    Ok(Some("null".into()))
                } else if n == 0.0 {
                    Ok(Some("0".into()))
                } else {
                    Ok(Some(number_to_string(n)))
                }
            }
            Kind::String(r) => Ok(Some(quote_json_str(self.vm.heap.string(r)))),
            Kind::Object(o) => {
                if self.stack.contains(&o) {
                    return Err(self.vm.type_error("Converting circular structure to JSON"));
                }
                self.stack.push(o);
                let is_array = matches!(self.vm.heap.object(o).kind, ObjectKind::Array(_));
                let res = if is_array {
                    self.serialize_array(o)
                } else {
                    self.serialize_object(o)
                };
                self.stack.pop();
                res.map(Some)
            }
            Kind::Undefined => Ok(None),
        }
    }

    fn serialize_array(&mut self, o: GcRef) -> Result<String, Value> {
        let len = match &self.vm.heap.object(o).kind {
            ObjectKind::Array(items) => items.len(),
            _ => 0,
        };
        if len == 0 {
            return Ok("[]".into());
        }
        let step_back = self.indent.clone();
        self.indent.push_str(&self.gap);
        let mut parts = Vec::with_capacity(len);
        for i in 0..len {
            let key = self.vm.heap.intern(&i.to_string());
            let item = self
                .serialize_prop(Value::object(o), key)?
                .unwrap_or_else(|| "null".into());
            parts.push(item);
        }
        let out = if self.gap.is_empty() {
            format!("[{}]", parts.join(","))
        } else {
            let sep = format!(",\n{}", self.indent);
            format!("[\n{}{}\n{}]", self.indent, parts.join(&sep), step_back)
        };
        self.indent = step_back;
        Ok(out)
    }

    fn serialize_object(&mut self, o: GcRef) -> Result<String, Value> {
        let keys: Vec<GcRef> = match &self.replacer {
            Replacer::List(list) => list.clone(),
            _ => self
                .vm
                .heap
                .object(o)
                .props
                .iter()
                .filter(|p| p.flags & PROP_ENUMERABLE != 0)
                .map(|p| p.key)
                .collect(),
        };
        let step_back = self.indent.clone();
        self.indent.push_str(&self.gap);
        let mut parts = Vec::new();
        for k in keys {
            let Some(val_str) = self.serialize_prop(Value::object(o), k)? else {
                continue;
            };
            let key_str = quote_json_str(self.vm.heap.string(k));
            if self.gap.is_empty() {
                parts.push(format!("{key_str}:{val_str}"));
            } else {
                parts.push(format!("{key_str}: {val_str}"));
            }
        }
        self.indent = step_back.clone();
        if parts.is_empty() {
            return Ok("{}".into());
        }
        if self.gap.is_empty() {
            Ok(format!("{{{}}}", parts.join(",")))
        } else {
            let sep = format!(",\n{}{}", step_back, self.gap);
            Ok(format!(
                "{{\n{}{}{}\n{}}}",
                step_back,
                self.gap,
                parts.join(&sep),
                step_back
            ))
        }
    }
}

pub(crate) fn quote_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
