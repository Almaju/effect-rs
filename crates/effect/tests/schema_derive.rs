//! Integration tests for `#[derive(Schema)]`.

use effect::Schema;
use effect::schema::serde_json::json;

#[derive(Debug, PartialEq, Schema)]
pub struct User {
    pub name: String,
    pub age: u32,
    pub nickname: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, PartialEq, Schema)]
pub struct Coords {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, PartialEq, Schema)]
pub struct Empty {}

#[test]
fn parses_full_record() {
    let input = json!({
        "name": "alice",
        "age": 30,
        "nickname": "al",
        "tags": ["admin"]
    });
    let parsed = User::parse_json(&input).unwrap();
    assert_eq!(
        parsed,
        User {
            name: "alice".into(),
            age: 30,
            nickname: Some("al".into()),
            tags: vec!["admin".into()],
        }
    );
}

#[test]
fn parses_with_optional_absent() {
    let input = json!({ "name": "bob", "age": 25, "tags": [] });
    let parsed = User::parse_json(&input).unwrap();
    assert_eq!(parsed.nickname, None);
}

#[test]
fn parses_with_optional_null() {
    let input = json!({ "name": "carol", "age": 40, "nickname": null, "tags": [] });
    let parsed = User::parse_json(&input).unwrap();
    assert_eq!(parsed.nickname, None);
}

#[test]
fn rejects_missing_required_field() {
    let input = json!({ "name": "no-age", "tags": [] });
    let err = User::parse_json(&input).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("'age'"), "got: {msg}");
}

#[test]
fn rejects_wrong_type_with_path() {
    let input = json!({ "name": "ok", "age": "not int", "tags": [] });
    let err = User::parse_json(&input).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("'age'"), "got: {msg}");
}

#[test]
fn nested_error_carries_path() {
    let input = json!({ "name": "ok", "age": 1, "tags": ["fine", 42] });
    let err = User::parse_json(&input).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("'tags'"), "got: {msg}");
    assert!(msg.contains("index 1"), "got: {msg}");
}

#[test]
fn encode_then_parse_roundtrips() {
    let original = User {
        name: "dan".into(),
        age: 55,
        nickname: Some("the dan".into()),
        tags: vec!["a".into(), "b".into()],
    };
    let encoded = original.encode_json();
    let reparsed = User::parse_json(&encoded).unwrap();
    assert_eq!(reparsed, original);
}

#[test]
fn json_schema_describes_object_with_required() {
    let spec = User::json_schema();
    assert_eq!(spec["type"], "object");
    let req = spec["required"].as_array().unwrap();
    let names: Vec<&str> = req.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"name"));
    assert!(names.contains(&"age"));
    assert!(names.contains(&"tags"));
    assert!(!names.contains(&"nickname")); // Option excluded
}

#[test]
fn smaller_struct_works() {
    let input = json!({ "x": 1.0, "y": 2.0 });
    let c = Coords::parse_json(&input).unwrap();
    assert_eq!(c, Coords { x: 1.0, y: 2.0 });
}

#[test]
fn empty_struct_works() {
    let input = json!({});
    let _: Empty = Empty::parse_json(&input).unwrap();
    let spec = Empty::json_schema();
    assert_eq!(spec["required"], json!([]));
}
