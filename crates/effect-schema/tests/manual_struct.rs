//! Hand-written `Schema` impl for a struct — proof-of-concept for what
//! `#[derive(Schema)]` will generate in 2e.

use effect_schema::{Schema, SchemaError};
use serde_json::{Value, json};

#[derive(Debug, PartialEq)]
pub struct User {
    pub name: String,
    pub age: u32,
    pub nickname: Option<String>,
    pub tags: Vec<String>,
}

impl Schema for User {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        let obj = input
            .as_object()
            .ok_or_else(|| SchemaError::type_mismatch("object", input))?;

        let name = <String as Schema>::parse_json(
            obj.get("name").ok_or_else(|| SchemaError::missing_field("name"))?,
        )
        .map_err(|e| SchemaError::at_field("name", e))?;

        let age = <u32 as Schema>::parse_json(
            obj.get("age").ok_or_else(|| SchemaError::missing_field("age"))?,
        )
        .map_err(|e| SchemaError::at_field("age", e))?;

        // Optional field: missing key → None.
        let nickname = match obj.get("nickname") {
            Some(v) => <Option<String> as Schema>::parse_json(v)
                .map_err(|e| SchemaError::at_field("nickname", e))?,
            None => None,
        };

        let tags = match obj.get("tags") {
            Some(v) => <Vec<String> as Schema>::parse_json(v)
                .map_err(|e| SchemaError::at_field("tags", e))?,
            None => Vec::new(),
        };

        Ok(User { name, age, nickname, tags })
    }

    fn encode_json(&self) -> Value {
        json!({
            "name":     self.name.encode_json(),
            "age":      self.age.encode_json(),
            "nickname": self.nickname.encode_json(),
            "tags":     self.tags.encode_json(),
        })
    }

    fn json_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name":     <String as Schema>::json_schema(),
                "age":      <u32 as Schema>::json_schema(),
                "nickname": <Option<String> as Schema>::json_schema(),
                "tags":     <Vec<String> as Schema>::json_schema(),
            },
            "required": ["name", "age"],
        })
    }
}

#[test]
fn parses_full_record() {
    let input = json!({
        "name": "alice",
        "age": 30,
        "nickname": "al",
        "tags": ["admin", "early-access"]
    });
    let parsed = User::parse_json(&input).unwrap();
    assert_eq!(
        parsed,
        User {
            name: "alice".into(),
            age: 30,
            nickname: Some("al".into()),
            tags: vec!["admin".into(), "early-access".into()],
        }
    );
}

#[test]
fn parses_with_optional_field_absent() {
    let input = json!({ "name": "bob", "age": 25 });
    let parsed = User::parse_json(&input).unwrap();
    assert_eq!(parsed.nickname, None);
    assert_eq!(parsed.tags, Vec::<String>::new());
}

#[test]
fn reports_path_to_failing_field() {
    let input = json!({ "name": "ok", "age": "not an integer" });
    let err = User::parse_json(&input).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("'age'"), "got: {msg}");
    assert!(msg.contains("integer") || msg.contains("string"), "got: {msg}");
}

#[test]
fn reports_path_through_nested_collections() {
    let input = json!({
        "name": "ok",
        "age": 1,
        "tags": ["fine", 42]
    });
    let err = User::parse_json(&input).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("'tags'"), "got: {msg}");
    assert!(msg.contains("index 1"), "got: {msg}");
}

#[test]
fn encode_roundtrips() {
    let u = User {
        name: "carol".into(),
        age: 40,
        nickname: None,
        tags: vec!["beta".into()],
    };
    let encoded = u.encode_json();
    let reparsed = User::parse_json(&encoded).unwrap();
    assert_eq!(reparsed, u);
}

#[test]
fn json_schema_describes_object() {
    let spec = User::json_schema();
    assert_eq!(spec["type"], "object");
    assert_eq!(spec["required"], json!(["name", "age"]));
    assert_eq!(spec["properties"]["name"]["type"], "string");
}
