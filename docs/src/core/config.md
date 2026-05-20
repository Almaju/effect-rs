# Configuration

`effect-config` is a thin layer over [`effect_schema`]: any
`T: Schema` is loadable from the process environment (or an in-memory
map) with one call.

```toml
[dependencies]
effect        = { version = "0.0.1" }
effect-config = { version = "0.0.1" }
```

## The shape

```rust,no_run
use effect::Schema;
use effect_config::from_env;

#[derive(Schema, Debug)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Schema, Debug)]
pub struct AppConfig {
    pub name: String,
    pub debug: bool,
    pub db: DbConfig,
}

# #[tokio::main] async fn main() {
// Env: APP_NAME=demo APP_DEBUG=true APP_DB__HOST=localhost APP_DB__PORT=5432
let cfg: AppConfig = from_env::<AppConfig>("APP_").execute().await.ok().unwrap();
# }
```

## Conventions

- **Prefix.** `from_env(prefix)` only considers env vars beginning
  with `prefix`. The prefix is stripped before nesting/coercion.
- **Nesting.** Keys are split on `__` (double underscore) to form
  nested object paths: `DB__HOST` → `{ "db": { "host": "..." } }`.
  Each path segment is lowercased.
- **Coercion.** Each raw string is coerced into a JSON value:
  - `"true"` / `"false"` → bool
  - `"42"`, `"-7"` → integer
  - `"3.14"` → float
  - anything else → string

After coercion the resulting object is parsed via `T::parse_json`,
which gives you the full power of Schema validation: required fields,
type checking, and per-field error paths.

## Errors

```rust,no_run
# use effect::Schema;
# use effect_config::from_env;
# #[derive(Schema, Debug)] pub struct C { host: String, port: u16 }
# #[tokio::main] async fn main() {
let result = from_env::<C>("APP_").execute().await;

match result {
    effect::Exit::Success(cfg) => { /* use cfg */ }
    effect::Exit::Failure(c) => eprintln!("config load failed: {c}"),
}
# }
```

Errors include the failing field path — e.g.
`schema parse error: at field 'db.port': type mismatch: expected integer, got string`.

## Testing — `from_map`

For tests, `from_map(HashMap<String, String>)` lets you build inputs
without touching the process environment:

```rust,no_run
use std::collections::HashMap;
use effect::Schema;
use effect_config::from_map;

#[derive(Schema, Debug)]
struct C { host: String, port: u16 }

# #[tokio::main] async fn main() {
let pairs: HashMap<String, String> = [
    ("host".to_string(), "localhost".to_string()),
    ("port".to_string(), "5432".to_string()),
]
.into_iter()
.collect();

let cfg: C = from_map::<C>(pairs).execute().await.ok().unwrap();
# }
```

## What's coming

- **`from_args`** — clap-style CLI integration that builds the same
  flat map.
- **`from_toml_file(path)` / `from_json_file(path)`** — file loaders
  with the same Schema-based parsing.
- **`from_layered([providers])`** — left-to-right precedence: file →
  env → CLI args overrides.
- **Refinements at the boundary** — auto-apply `#[derive(Brand)]`
  constructors when loading, so the parsed value is already
  validated.
- **Watch & reload** — `Effect::repeat` + a file watcher to emit a
  fresh config stream.

[`effect_schema`]: ../data/schemas.md
