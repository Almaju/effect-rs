# Schemas

A `Schema` is one declaration that powers three things:

1. **Parse** a JSON value into a typed Rust value, with structured
   errors that name the failing field path.
2. **Encode** a typed Rust value back to JSON.
3. **Introspect** — emit a JSON Schema (Draft 2020-12) describing the
   shape, suitable for OpenAPI docs, generators, or runtime validation.

`#[derive(Schema)]` writes the impl for you.

```rust,no_run
use effect::Schema;
use effect::schema::serde_json::json;

#[derive(Debug, Schema)]
pub struct User {
    pub name: String,
    pub age: u32,
    pub nickname: Option<String>,
    pub tags: Vec<String>,
}

# fn main() -> Result<(), effect::SchemaError> {
let input = json!({
    "name": "alice",
    "age": 30,
    "tags": ["admin"]
});

let parsed: User = User::parse_json(&input)?;
let back   = parsed.encode_json();
let spec   = User::json_schema();    // serde_json::Value of JSON Schema
# Ok(()) }
```

## The trait

```rust,ignore
pub trait Schema: Sized {
    fn parse_json(input: &Value)  -> Result<Self, SchemaError>;
    fn encode_json(&self)         -> Value;
    fn json_schema()              -> Value;   // JSON Schema (Draft 2020-12)
}
```

Built-in impls cover:

| Type      | JSON                       | JSON Schema                        |
| --------- | -------------------------- | ---------------------------------- |
| `bool`    | `true` / `false`           | `{ "type": "boolean" }`            |
| `String`  | string                     | `{ "type": "string" }`             |
| `char`    | single-char string         | `{ "type": "string", "minLength": 1, "maxLength": 1 }` |
| `i8..i64`, `isize` | integer           | `{ "type": "integer", "format": "intN" }` |
| `u8..u64`, `usize` | non-negative int  | `{ "type": "integer", "format": "uintN", "minimum": 0 }` |
| `f32`, `f64` | number                  | `{ "type": "number", "format": "float"/"double" }` |
| `Option<T>` | `null` or `T`            | `{ "anyOf": [{ "type": "null" }, T] }` |
| `Vec<T>`  | array                      | `{ "type": "array", "items": T }`  |

## `#[derive(Schema)]`

For named-field structs:

```rust,no_run
use effect::Schema;

#[derive(Schema)]
pub struct Address {
    pub street: String,
    pub city:   String,
    pub zip:    Option<String>,
}
```

What's generated:

```rust,ignore
impl effect::Schema for Address {
    fn parse_json(input: &Value) -> Result<Self, SchemaError> {
        let obj = input.as_object()
            .ok_or_else(|| SchemaError::type_mismatch("object", input))?;
        Ok(Address {
            street: <String as Schema>::parse_json(
                obj.get("street").ok_or_else(|| SchemaError::missing_field("street"))?
            ).map_err(|e| SchemaError::at_field("street", e))?,
            city:   /* same pattern */,
            zip:    match obj.get("zip") {
                Some(v) => <Option<String> as Schema>::parse_json(v)
                    .map_err(|e| SchemaError::at_field("zip", e))?,
                None    => None,
            },
        })
    }
    fn encode_json(&self) -> Value { /* JSON object with all fields */ }
    fn json_schema() -> Value     { /* type: object, properties, required */ }
}
```

Notes:
- **`Option<T>` fields are optional.** Missing keys parse as `None`;
  `null` also parses as `None`. They're excluded from `required` in
  the JSON Schema output.
- **All other fields are required.** A missing key produces
  `SchemaError::MissingField`.
- **Errors carry the field path.** A type mismatch in `Address.zip`
  reports as `at field 'zip': type mismatch: expected string, got number`.
  Nested errors in `Vec<T>` also carry index — `at field 'tags': at index 2: …`.

## Structured errors

```rust,no_run
use effect::{Schema, SchemaError};

# #[derive(Schema, Debug)] struct User { name: String, age: u32 }
# fn main() {
let err = User::parse_json(&effect::schema::serde_json::json!({ "name": "ok" })).unwrap_err();
println!("{err}");   // missing required field 'age'

match err {
    SchemaError::MissingField(f) => println!("missing: {f}"),
    SchemaError::TypeMismatch { expected, actual } => {
        println!("expected {expected}, got {actual}")
    }
    SchemaError::AtField { field, source } => println!("in {field}: {source}"),
    SchemaError::AtIndex { index, source } => println!("at [{index}]: {source}"),
    SchemaError::Invalid(msg) => println!("invalid: {msg}"),
}
# }
```

## Producing a JSON Schema document

`json_schema()` returns a `serde_json::Value` matching JSON Schema
Draft 2020-12. Print it, attach it to an OpenAPI spec, or feed it to a
validator at runtime:

```rust,no_run
# use effect::Schema;
# #[derive(Schema)] struct User { name: String, age: u32 }
# fn main() {
let spec = User::json_schema();
println!("{}", effect::schema::serde_json::to_string_pretty(&spec).unwrap());
//   {
//     "type": "object",
//     "properties": {
//       "name": { "type": "string" },
//       "age":  { "type": "integer", "format": "uint32", "minimum": 0 }
//     },
//     "required": ["name", "age"]
//   }
# }
```

## Re-exported `serde_json`

`effect-schema` re-exports `serde_json` (as `effect::schema::serde_json`)
so users don't have to declare it separately:

```rust,no_run
use effect::schema::serde_json::{json, Value};

let v: Value = json!({ "ok": true });
```

If you already have `serde_json` in your `Cargo.toml` for other
reasons, use it directly — both paths point at the same crate.

## What's coming

- **Enum derive** — `#[derive(Schema)]` for tagged unions
  (`#[schema(tag = "kind")]`).
- **Field attributes** — `#[schema(rename = "user_name")]`,
  `#[schema(default = "…")]`.
- **Transformations** — `Schema::transform(parse, encode)` to derive
  one schema from another (e.g. ISO-8601 string → `DateTime`).
- **Refinement integration** — composing a `Refinement` with a base
  schema for runtime validation on parse.
- **serde adapter** — bridge `Schema` to existing
  `Serialize`/`Deserialize` derives for interop.
- **Brand-aware schemas** — auto-derive `Schema` for `#[derive(Brand)]`
  newtypes by composing the inner schema with `try_new`.
