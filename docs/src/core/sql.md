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

## Writing a real driver

Until per-driver crates ship, here's the sketch. For an `sqlx::SqlitePool`:

```rust,ignore
use effect_sql::*;
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Column, Row as _};

pub struct SqliteExecutor(pub SqlitePool);

impl SqlExecutor for SqliteExecutor {
    fn execute(&self, sql: String, params: Vec<SqlValue>) -> AsyncResult<Result<u64, SqlError>> {
        let pool = self.0.clone();
        Box::pin(async move {
            let mut q = sqlx::query(&sql);
            for p in params { q = bind_value(q, p); }
            q.execute(&pool).await
                .map(|r| r.rows_affected())
                .map_err(|e| SqlError::Database(e.to_string()))
        })
    }
    // fetch_all: similar; iterate columns and convert SqliteRow → Row.
}
```

A future `effect-sql-sqlite` crate will ship this + a connection pool
constructor.

## What's coming

- **`effect-sql-sqlite`** — sqlx-backed driver. SQLite first (no
  external server, perfect for tests).
- **`effect-sql-postgres`** — Postgres driver.
- **Schema-typed rows** — `fetch_one_as::<T: Schema>(sql, params)` for
  typed row → struct decoding.
- **Transactions** — `db.transaction(|tx| async { … })`.
- **Migrations** — Schema-driven migrations.
- **Query DSL** — Effect-typed query builder so the SQL string isn't
  the only escape hatch.
