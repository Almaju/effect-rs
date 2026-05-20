# SQL

`effect-sql` is the abstract `SqlExecutor` trait + Effect-wrapped
helpers. Per-database drivers (`effect-sql-sqlite`,
`effect-sql-postgres`, …) live in separate crates and plug into this
trait — none ship yet; until they do, you can write your own driver
impl in ~30 lines.

```toml
[dependencies]
effect     = { version = "0.0.1" }
effect-sql = { version = "0.0.1" }
```

## The trait + helpers

```rust,ignore
pub trait SqlExecutor: Send + Sync + 'static {
    fn execute(&self, sql: String, params: Vec<SqlValue>)
        -> AsyncResult<Result<u64, SqlError>>;       // rows affected
    fn fetch_all(&self, sql: String, params: Vec<SqlValue>)
        -> AsyncResult<Result<Vec<Row>, SqlError>>;
}
```

Free helpers that produce Effects:

| Helper                 | Returns                                      |
| ---------------------- | -------------------------------------------- |
| `execute(sql, params)` | `Effect<u64, SqlError, R: SqlExecutor>`      |
| `fetch_all(sql, p)`    | `Effect<Vec<Row>, SqlError, R>`              |
| `fetch_one(sql, p)`    | `Effect<Row, SqlError, R>` (errors on ≠ 1)   |
| `fetch_optional(sql,p)`| `Effect<Option<Row>, SqlError, R>`           |

A `Row` is a `HashMap<String, SqlValue>` — columns by name. `SqlValue`
is a loose enum (`Null` / `Bool` / `Int(i64)` / `Float(f64)` /
`Text(String)` / `Bytes(Vec<u8>)`) with `as_*` accessors.

```rust,no_run
use effect_sql::{fetch_all, FakeSqlExecutor, SqlValue, row};

# #[tokio::main] async fn main() {
let db = FakeSqlExecutor::new();
db.expect_fetch(
    "SELECT name, age FROM users WHERE active = ?",
    vec![
        row([("name", "alice"), ("age", "30")]),
        row([("name", "bob"),   ("age", "25")]),
    ],
);

let rows = fetch_all::<FakeSqlExecutor>(
    "SELECT name, age FROM users WHERE active = ?",
    vec![SqlValue::Bool(true)],
)
.run_with(db)
.await
.ok()
.unwrap();

assert_eq!(rows.len(), 2);
assert_eq!(rows[0].get("name").unwrap().as_text(), Some("alice"));
# }
```

## Errors

```rust,ignore
pub enum SqlError {
    Database(String),
    UnexpectedRowCount { got: usize },   // fetch_one with 0 or >1 rows
    MissingColumn(String),
    TypeMismatch { column: String, expected: String },
    DriverUnavailable(String),
}
```

## Testing — `FakeSqlExecutor`

```rust,no_run
use effect_sql::*;

# #[tokio::main] async fn main() {
let db = FakeSqlExecutor::new();
db.expect_execute("INSERT INTO users (name) VALUES (?)", 1);
db.expect_fetch(
    "SELECT * FROM users WHERE id = ?",
    vec![row([("id", SqlValue::Int(1)), ("name", SqlValue::Text("alice".into()))])],
);

let arc = std::sync::Arc::new(db);
let _ = execute::<FakeSqlExecutor>(
    "INSERT INTO users (name) VALUES (?)",
    vec![SqlValue::Text("alice".into())],
).run(arc.clone()).await;

// Inspect what was sent.
let log = arc.recorded();
assert_eq!(log.len(), 1);
assert_eq!(log[0].1[0], SqlValue::Text("alice".into()));
# }
```

## SQLite driver — `effect-sql-sqlite`

The companion crate `effect-sql-sqlite` wraps `sqlx::SqlitePool`. Add
it alongside `effect-sql`:

```toml
[dependencies]
effect-sql        = { version = "0.0.1" }
effect-sql-sqlite = { version = "0.0.1" }
```

```rust,no_run
use effect_sql::{execute, fetch_all, SqlValue};
use effect_sql_sqlite::SqliteExecutor;

# #[tokio::main] async fn main() {
let db = SqliteExecutor::connect("sqlite::memory:").await.unwrap();

let _ = execute::<SqliteExecutor>(
    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
    vec![],
).run_with(db).await;
# }
```

`SqliteExecutor::connect(url)` opens the database (use
`"sqlite::memory:"` for an ephemeral in-memory DB).
`SqliteExecutor::with_pool(pool)` wraps an existing
`sqlx::SqlitePool` if you already have one.

Decoding: SQLite stores values dynamically — the driver tries `i64`,
`f64`, `String`, `Vec<u8>`, `bool` in order and picks the first that
succeeds.

## What's coming

- **`effect-sql-postgres`** — Postgres driver, same shape.
- **Schema-typed rows** — `fetch_one_as::<T: Schema>(sql, params)` for
  typed row → struct decoding.
- **Transactions** — `db.transaction(|tx| async { … })`.
- **Migrations** — Schema-driven migrations.
- **Query DSL** — Effect-typed query builder so the SQL string isn't
  the only escape hatch.
