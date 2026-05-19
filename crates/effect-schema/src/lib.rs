//! [`Schema`] — a single declaration that supports parsing, encoding,
//! and JSON Schema introspection.
//!
//! ```ignore
//! pub struct User { name: String, age: u32 }
//!
//! impl Schema for User { /* derive in 2e */ }
//!
//! let json    = serde_json::json!({ "name": "alice", "age": 30 });
//! let parsed  = User::parse_json(&json)?;     // Result<User, SchemaError>
//! let encoded = parsed.encode_json();         // serde_json::Value
//! let spec    = User::json_schema();          // JSON Schema as a Value
//! ```
//!
//! Built-in `Schema` impls cover the integer/float/string/bool/char
//! primitives, plus `Option<T>` and `Vec<T>` as containers. Implement
//! `Schema` by hand for a custom struct in 2d; in 2e the
//! `#[derive(Schema)]` macro will do it for you.

pub mod error;
pub mod primitives;

pub use error::SchemaError;

use serde_json::Value;

/// A value type that knows how to parse from / encode to JSON and how
/// to describe itself as JSON Schema.
pub trait Schema: Sized {
    /// Parse a value from a JSON node. Returns a structured
    /// [`SchemaError`] on mismatch.
    fn parse_json(input: &Value) -> Result<Self, SchemaError>;

    /// Encode `self` to a JSON node.
    fn encode_json(&self) -> Value;

    /// Describe this schema as JSON Schema (Draft 2020-12 shape).
    fn json_schema() -> Value;
}

// ── Container impls ────────────────────────────────────────────────

impl<T: Schema> Schema for Option<T> {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        if input.is_null() {
            Ok(None)
        } else {
            T::parse_json(input).map(Some)
        }
    }
    fn encode_json(&self) -> Value {
        match self {
            Some(v) => v.encode_json(),
            None => Value::Null,
        }
    }
    fn json_schema() -> Value {
        serde_json::json!({
            "anyOf": [
                { "type": "null" },
                T::json_schema(),
            ],
        })
    }
}

impl<T: Schema> Schema for Vec<T> {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        let arr = input.as_array().ok_or_else(|| SchemaError::type_mismatch("array", input))?;
        let mut out = Vec::with_capacity(arr.len());
        for (index, item) in arr.iter().enumerate() {
            match T::parse_json(item) {
                Ok(v) => out.push(v),
                Err(source) => return Err(SchemaError::at_index(index, source)),
            }
        }
        Ok(out)
    }
    fn encode_json(&self) -> Value {
        Value::Array(self.iter().map(T::encode_json).collect())
    }
    fn json_schema() -> Value {
        serde_json::json!({
            "type": "array",
            "items": T::json_schema(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn option_some_roundtrips() {
        let parsed: Option<i32> = <Option<i32> as Schema>::parse_json(&json!(42)).unwrap();
        assert_eq!(parsed, Some(42));
        assert_eq!(parsed.encode_json(), json!(42));
    }

    #[test]
    fn option_null_parses_as_none() {
        let parsed: Option<i32> = <Option<i32> as Schema>::parse_json(&json!(null)).unwrap();
        assert_eq!(parsed, None);
        assert_eq!(parsed.encode_json(), json!(null));
    }

    #[test]
    fn vec_roundtrips() {
        let parsed: Vec<i32> = Schema::parse_json(&json!([1, 2, 3])).unwrap();
        assert_eq!(parsed, vec![1, 2, 3]);
        assert_eq!(parsed.encode_json(), json!([1, 2, 3]));
    }

    #[test]
    fn vec_rejects_non_array() {
        let err = <Vec<i32> as Schema>::parse_json(&json!("not an array")).unwrap_err();
        assert!(matches!(err, SchemaError::TypeMismatch { .. }));
    }

    #[test]
    fn vec_reports_failing_index() {
        let err = <Vec<i32> as Schema>::parse_json(&json!([1, "bad", 3])).unwrap_err();
        match err {
            SchemaError::AtIndex { index, .. } => assert_eq!(index, 1),
            other => panic!("expected AtIndex, got {other:?}"),
        }
    }

    #[test]
    fn json_schema_for_option_is_anyof_null_plus_inner() {
        let spec = <Option<i32> as Schema>::json_schema();
        assert!(spec.get("anyOf").is_some());
    }

    #[test]
    fn json_schema_for_vec_is_array_of_inner() {
        let spec = <Vec<i32> as Schema>::json_schema();
        assert_eq!(spec["type"], "array");
        assert_eq!(spec["items"]["type"], "integer");
    }
}
