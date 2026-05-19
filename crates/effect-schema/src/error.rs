//! [`SchemaError`] — structured failure for schema parsing.
//!
//! Errors compose via [`SchemaError::at_field`] and
//! [`SchemaError::at_index`] so the final message names the path to the
//! offending node, like `at 'users.0.email': type mismatch`.

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        expected: String,
        actual: String,
    },

    #[error("missing required field '{0}'")]
    MissingField(String),

    #[error("invalid value: {0}")]
    Invalid(String),

    #[error("at field '{field}': {source}")]
    AtField {
        field: String,
        #[source]
        source: Box<SchemaError>,
    },

    #[error("at index {index}: {source}")]
    AtIndex {
        index: usize,
        #[source]
        source: Box<SchemaError>,
    },
}

impl SchemaError {
    pub fn type_mismatch(expected: impl Into<String>, actual: &Value) -> Self {
        SchemaError::TypeMismatch {
            expected: expected.into(),
            actual: describe_value(actual).to_string(),
        }
    }

    pub fn missing_field(name: impl Into<String>) -> Self {
        SchemaError::MissingField(name.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        SchemaError::Invalid(message.into())
    }

    pub fn at_field(field: impl Into<String>, source: SchemaError) -> Self {
        SchemaError::AtField {
            field: field.into(),
            source: Box::new(source),
        }
    }

    pub fn at_index(index: usize, source: SchemaError) -> Self {
        SchemaError::AtIndex {
            index,
            source: Box::new(source),
        }
    }
}

fn describe_value(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
