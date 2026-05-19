//! `Schema` impls for the primitive types.

use serde_json::{Number, Value, json};

use crate::{Schema, SchemaError};

impl Schema for bool {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        input
            .as_bool()
            .ok_or_else(|| SchemaError::type_mismatch("bool", input))
    }
    fn encode_json(&self) -> Value {
        Value::Bool(*self)
    }
    fn json_schema() -> Value {
        json!({ "type": "boolean" })
    }
}

impl Schema for String {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        input
            .as_str()
            .map(String::from)
            .ok_or_else(|| SchemaError::type_mismatch("string", input))
    }
    fn encode_json(&self) -> Value {
        Value::String(self.clone())
    }
    fn json_schema() -> Value {
        json!({ "type": "string" })
    }
}

impl Schema for char {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        let s = input
            .as_str()
            .ok_or_else(|| SchemaError::type_mismatch("string (char)", input))?;
        let mut chars = s.chars();
        let first = chars
            .next()
            .ok_or_else(|| SchemaError::invalid("expected a single character, got empty string"))?;
        if chars.next().is_some() {
            return Err(SchemaError::invalid(
                "expected a single character, got multiple",
            ));
        }
        Ok(first)
    }
    fn encode_json(&self) -> Value {
        Value::String(self.to_string())
    }
    fn json_schema() -> Value {
        json!({ "type": "string", "minLength": 1, "maxLength": 1 })
    }
}

// ── Integers ──────────────────────────────────────────────────────

macro_rules! impl_int {
    ($($t:ty as $kind:expr, $format:expr),* $(,)?) => {
        $(
            impl Schema for $t {
                fn parse_json(input: &Value) -> Result<Self, SchemaError> {
                    input
                        .as_i64()
                        .and_then(|n| <$t>::try_from(n).ok())
                        .ok_or_else(|| SchemaError::type_mismatch($kind, input))
                }
                fn encode_json(&self) -> Value {
                    Value::Number((*self).into())
                }
                fn json_schema() -> Value {
                    json!({ "type": "integer", "format": $format })
                }
            }
        )*
    };
}
impl_int! {
    i8    as "i8",    "int8",
    i16   as "i16",   "int16",
    i32   as "i32",   "int32",
    i64   as "i64",   "int64",
    isize as "isize", "isize",
    u8    as "u8",    "uint8",
    u16   as "u16",   "uint16",
    u32   as "u32",   "uint32",
}

// u64 and usize need extra care because as_i64 can lose values > i64::MAX.
impl Schema for u64 {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        input
            .as_u64()
            .ok_or_else(|| SchemaError::type_mismatch("u64", input))
    }
    fn encode_json(&self) -> Value {
        Value::Number((*self).into())
    }
    fn json_schema() -> Value {
        json!({ "type": "integer", "format": "uint64", "minimum": 0 })
    }
}

impl Schema for usize {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        let n = input
            .as_u64()
            .ok_or_else(|| SchemaError::type_mismatch("usize", input))?;
        usize::try_from(n)
            .map_err(|_| SchemaError::invalid("value exceeds platform usize"))
    }
    fn encode_json(&self) -> Value {
        Value::Number((*self as u64).into())
    }
    fn json_schema() -> Value {
        json!({ "type": "integer", "format": "usize", "minimum": 0 })
    }
}

// ── Floats ────────────────────────────────────────────────────────

impl Schema for f32 {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        input
            .as_f64()
            .map(|n| n as f32)
            .ok_or_else(|| SchemaError::type_mismatch("number", input))
    }
    fn encode_json(&self) -> Value {
        Number::from_f64(*self as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
    fn json_schema() -> Value {
        json!({ "type": "number", "format": "float" })
    }
}

impl Schema for f64 {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        input
            .as_f64()
            .ok_or_else(|| SchemaError::type_mismatch("number", input))
    }
    fn encode_json(&self) -> Value {
        Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
    fn json_schema() -> Value {
        json!({ "type": "number", "format": "double" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bool_roundtrips() {
        let v: bool = Schema::parse_json(&json!(true)).unwrap();
        assert!(v);
        assert_eq!(v.encode_json(), json!(true));
    }

    #[test]
    fn string_roundtrips() {
        let v: String = Schema::parse_json(&json!("hi")).unwrap();
        assert_eq!(v, "hi");
        assert_eq!(v.encode_json(), json!("hi"));
    }

    #[test]
    fn i32_roundtrips() {
        let v: i32 = Schema::parse_json(&json!(42)).unwrap();
        assert_eq!(v, 42);
        assert_eq!(v.encode_json(), json!(42));
    }

    #[test]
    fn i32_overflow_is_rejected() {
        let v: Result<i32, _> = Schema::parse_json(&json!(i64::MAX));
        assert!(v.is_err());
    }

    #[test]
    fn u64_handles_large_values() {
        let v: u64 = Schema::parse_json(&json!(u64::MAX)).unwrap();
        assert_eq!(v, u64::MAX);
    }

    #[test]
    fn f64_roundtrips() {
        let v: f64 = Schema::parse_json(&json!(1.5)).unwrap();
        assert_eq!(v, 1.5);
        assert_eq!(v.encode_json(), json!(1.5));
    }

    #[test]
    fn char_roundtrips_single_char() {
        let v: char = Schema::parse_json(&json!("Q")).unwrap();
        assert_eq!(v, 'Q');
        assert_eq!(v.encode_json(), json!("Q"));
    }

    #[test]
    fn char_rejects_multi_char_string() {
        let v: Result<char, _> = Schema::parse_json(&json!("AB"));
        assert!(v.is_err());
    }

    #[test]
    fn type_mismatch_reports_actual() {
        let err = <i32 as Schema>::parse_json(&json!("oops")).unwrap_err();
        assert!(format!("{err}").contains("string"));
    }

    #[test]
    fn json_schema_for_string_is_string_type() {
        assert_eq!(<String as Schema>::json_schema()["type"], "string");
    }

    #[test]
    fn json_schema_for_i64_is_int_with_format() {
        let s = <i64 as Schema>::json_schema();
        assert_eq!(s["type"], "integer");
        assert_eq!(s["format"], "int64");
    }
}
