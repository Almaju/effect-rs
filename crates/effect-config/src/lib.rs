//! Declarative configuration on top of [`effect_schema`].
//!
//! A configuration type is any `T: Schema`. Loaders read key/value
//! pairs (env vars or an in-memory map), coerce string values to
//! their JSON-typed counterparts (`"true"` → bool, `"42"` → integer,
//! `"3.14"` → float, else string), and parse via `T::parse_json`.
//!
//! Nested fields are encoded as `PREFIX__FIELD` — the double
//! underscore acts as a path separator.
//!
//! ```ignore
//! use effect::Schema;
//! use effect_config::from_env;
//!
//! #[derive(Schema, Debug)]
//! struct DbConfig {
//!     host: String,
//!     port: u16,
//! }
//!
//! #[derive(Schema, Debug)]
//! struct AppConfig {
//!     name: String,
//!     debug: bool,
//!     db: DbConfig,
//! }
//!
//! // With env: APP_NAME=demo APP_DEBUG=true APP_DB__HOST=localhost APP_DB__PORT=5432
//! let cfg: AppConfig = from_env::<AppConfig>("APP_").execute().await.ok().unwrap();
//! ```

use std::collections::HashMap;

use effect::Effect;
use effect_schema::{Schema, SchemaError};
use effect_schema::serde_json::{Number, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("schema parse error: {0}")]
    Parse(#[from] SchemaError),

    #[error("env reading is unsupported in this environment: {0}")]
    EnvUnavailable(String),
}

/// Load configuration of type `T` from the process environment.
///
/// Only env vars beginning with `prefix` are considered (case-sensitive).
/// The prefix is stripped, and the remaining key is split on `__` to
/// form nested object paths. Values are coerced to bool / integer /
/// float / string.
pub fn from_env<T: Schema + Send + 'static>(
    prefix: &'static str,
) -> Effect<T, ConfigError, ()> {
    Effect::from_fn(move |_| async move {
        let map: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k[prefix.len()..].to_string(), v))
            .collect();
        load_from_map::<T>(&map)
    })
}

/// Load configuration from an in-memory map. Useful for tests and
/// alternate sources (TOML/YAML files mapped to flat keys).
pub fn from_map<T: Schema + Send + 'static>(
    map: HashMap<String, String>,
) -> Effect<T, ConfigError, ()> {
    Effect::from_fn(move |_| {
        let map = map.clone();
        async move { load_from_map::<T>(&map) }
    })
}

fn load_from_map<T: Schema>(map: &HashMap<String, String>) -> Result<T, ConfigError> {
    let value = build_json(map);
    T::parse_json(&value).map_err(ConfigError::Parse)
}

/// Build a nested `serde_json::Value` from a flat key/value map. Keys
/// are split on `__` to determine nesting; values are coerced.
pub fn build_json(map: &HashMap<String, String>) -> Value {
    let mut root = effect_schema::serde_json::Map::<String, Value>::new();
    for (key, raw) in map {
        let path: Vec<String> = key.split("__").map(|s| s.to_lowercase()).collect();
        insert_at_path(&mut root, &path, coerce(raw));
    }
    Value::Object(root)
}

fn insert_at_path(
    obj: &mut effect_schema::serde_json::Map<String, Value>,
    path: &[String],
    value: Value,
) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        obj.insert(path[0].clone(), value);
        return;
    }
    let head = &path[0];
    let entry = obj
        .entry(head.clone())
        .or_insert_with(|| Value::Object(effect_schema::serde_json::Map::new()));
    if let Value::Object(inner) = entry {
        insert_at_path(inner, &path[1..], value);
    } else {
        // Collision: a leaf is being overwritten with a nested entry.
        // Replace with a fresh object.
        let mut fresh = effect_schema::serde_json::Map::new();
        insert_at_path(&mut fresh, &path[1..], value);
        *entry = Value::Object(fresh);
    }
}

/// Coerce a raw env string into the best-fit JSON value.
pub fn coerce(s: &str) -> Value {
    if let Ok(b) = s.parse::<bool>() {
        return Value::Bool(b);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return Value::Number(n.into());
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = Number::from_f64(n) {
            return Value::Number(num);
        }
    }
    Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Schema;

    #[derive(Debug, PartialEq, Schema)]
    struct DbCfg {
        host: String,
        port: u16,
    }

    #[derive(Debug, PartialEq, Schema)]
    struct AppCfg {
        name: String,
        debug: bool,
        db: DbCfg,
    }

    fn map_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn coerce_recognizes_bool() {
        assert_eq!(coerce("true"), Value::Bool(true));
        assert_eq!(coerce("false"), Value::Bool(false));
    }

    #[test]
    fn coerce_recognizes_integers() {
        assert_eq!(coerce("42"), Value::Number(42.into()));
        assert_eq!(coerce("-7"), Value::Number((-7_i64).into()));
    }

    #[test]
    fn coerce_recognizes_floats() {
        assert_eq!(coerce("1.5"), Value::Number(Number::from_f64(1.5).unwrap()));
    }

    #[test]
    fn coerce_falls_back_to_string() {
        assert_eq!(coerce("hello"), Value::String("hello".to_string()));
    }

    #[test]
    fn build_json_handles_nested_paths() {
        let map = map_of(&[
            ("name", "demo"),
            ("debug", "true"),
            ("db__host", "localhost"),
            ("db__port", "5432"),
        ]);
        let json = build_json(&map);
        assert_eq!(json["name"], Value::String("demo".into()));
        assert_eq!(json["debug"], Value::Bool(true));
        assert_eq!(json["db"]["host"], Value::String("localhost".into()));
        assert_eq!(json["db"]["port"], Value::Number(5432.into()));
    }

    #[tokio::test]
    async fn from_map_loads_nested_schema() {
        let map = map_of(&[
            ("name", "demo"),
            ("debug", "true"),
            ("db__host", "localhost"),
            ("db__port", "5432"),
        ]);
        let exit = from_map::<AppCfg>(map).execute().await;
        let cfg = exit.ok().unwrap();
        assert_eq!(
            cfg,
            AppCfg {
                name: "demo".into(),
                debug: true,
                db: DbCfg {
                    host: "localhost".into(),
                    port: 5432,
                }
            }
        );
    }

    #[tokio::test]
    async fn from_map_reports_missing_required_field() {
        let map = map_of(&[("name", "demo"), ("debug", "true")]);
        let exit = from_map::<AppCfg>(map).execute().await;
        let err = exit.err().unwrap();
        let msg = format!("{err}");
        assert!(msg.contains("'db'") || msg.contains("missing"), "got: {msg}");
    }

    #[tokio::test]
    async fn from_map_reports_type_mismatch_with_field_path() {
        let map = map_of(&[
            ("name", "demo"),
            ("debug", "not-a-bool"),  // coerces to string, schema expects bool
            ("db__host", "h"),
            ("db__port", "1"),
        ]);
        let exit = from_map::<AppCfg>(map).execute().await;
        let err = exit.err().unwrap();
        let msg = format!("{err}");
        assert!(msg.contains("'debug'"), "got: {msg}");
    }
}
