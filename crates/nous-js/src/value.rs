//! Valores JavaScript em 64 bits (NaN-boxing).
//!
//! Um número é guardado como o próprio `f64`. Os outros tipos moram dentro de padrões
//! de bits de NaN que nunca aparecem em contas reais (o NaN de verdade é sempre
//! normalizado para um único padrão). Resultado: todo valor cabe em 8 bytes, sem
//! alocação para números, e o teste de tipo é uma comparação de inteiros.

use crate::heap::GcRef;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const TAG_SPECIAL: u64 = 0xFFF9_0000_0000_0000;
const TAG_STRING: u64 = 0xFFFA_0000_0000_0000;
const TAG_OBJECT: u64 = 0xFFFB_0000_0000_0000;
const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Value(u64);

/// Forma "aberta" de um [`Value`], para `match`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(GcRef),
    Object(GcRef),
}

impl Value {
    pub const UNDEFINED: Value = Value(TAG_SPECIAL);
    pub const NULL: Value = Value(TAG_SPECIAL | 1);
    pub const FALSE: Value = Value(TAG_SPECIAL | 2);
    pub const TRUE: Value = Value(TAG_SPECIAL | 3);

    #[inline]
    pub fn number(n: f64) -> Value {
        if n.is_nan() {
            Value(CANONICAL_NAN)
        } else {
            Value(n.to_bits())
        }
    }

    #[inline]
    pub fn boolean(b: bool) -> Value {
        if b { Value::TRUE } else { Value::FALSE }
    }

    #[inline]
    pub fn string(r: GcRef) -> Value {
        Value(TAG_STRING | r.0 as u64)
    }

    #[inline]
    pub fn object(r: GcRef) -> Value {
        Value(TAG_OBJECT | r.0 as u64)
    }

    #[inline]
    pub fn is_number(self) -> bool {
        self.0 < TAG_SPECIAL
    }

    #[inline]
    pub fn as_number(self) -> Option<f64> {
        self.is_number().then(|| f64::from_bits(self.0))
    }

    #[inline]
    pub fn as_string(self) -> Option<GcRef> {
        (self.0 & TAG_MASK == TAG_STRING).then_some(GcRef(self.0 as u32))
    }

    #[inline]
    pub fn as_object(self) -> Option<GcRef> {
        (self.0 & TAG_MASK == TAG_OBJECT).then_some(GcRef(self.0 as u32))
    }

    #[inline]
    pub fn is_undefined(self) -> bool {
        self.0 == Value::UNDEFINED.0
    }

    #[inline]
    pub fn is_nullish(self) -> bool {
        self.0 == Value::UNDEFINED.0 || self.0 == Value::NULL.0
    }

    #[inline]
    pub fn kind(self) -> Kind {
        if self.is_number() {
            return Kind::Number(f64::from_bits(self.0));
        }
        match self.0 & TAG_MASK {
            TAG_STRING => Kind::String(GcRef(self.0 as u32)),
            TAG_OBJECT => Kind::Object(GcRef(self.0 as u32)),
            _ => match self.0 & 0xF {
                0 => Kind::Undefined,
                1 => Kind::Null,
                2 => Kind::Bool(false),
                _ => Kind::Bool(true),
            },
        }
    }

    /// Identidade dos bits (não é igualdade JavaScript).
    #[inline]
    pub fn same_bits(self, other: Value) -> bool {
        self.0 == other.0
    }

    /// Os 64 bits crus (usado para deduplicar constantes no compilador).
    #[inline]
    pub fn to_bits(self) -> u64 {
        self.0
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.kind())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caixas() {
        assert_eq!(Value::number(1.5).kind(), Kind::Number(1.5));
        assert_eq!(
            Value::number(-0.0).as_number().unwrap().to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(
            Value::number(f64::NEG_INFINITY).kind(),
            Kind::Number(f64::NEG_INFINITY)
        );
        assert!(Value::number(f64::NAN).as_number().unwrap().is_nan());
        assert!(Value::number(-f64::NAN).is_number());
        assert_eq!(Value::UNDEFINED.kind(), Kind::Undefined);
        assert_eq!(Value::NULL.kind(), Kind::Null);
        assert_eq!(Value::TRUE.kind(), Kind::Bool(true));
        assert_eq!(Value::string(GcRef(7)).kind(), Kind::String(GcRef(7)));
        assert_eq!(
            Value::object(GcRef(u32::MAX)).kind(),
            Kind::Object(GcRef(u32::MAX))
        );
        assert_eq!(std::mem::size_of::<Value>(), 8);
    }
}
