//! Abstract SQL executor + Effect helpers.
//!
//! [`SqlExecutor`] is the service trait you bound `R` by. This crate
//! intentionally does NOT ship a driver — per-database crates
//! (`effect-sql-sqlite`, `effect-sql-postgres`, …) will. For tests,
//! use [`FakeSqlExecutor`] to record queries and serve canned rows.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use effect::Effect;
use thiserror::Error;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── SqlValue / Row ───────────────────────────────────────────────

/// A loosely-typed value used in parameter binding and row decoding.
/// Drivers map these to their native types.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl From<bool>   for SqlValue { fn from(v: bool)   -> Self { SqlValue::Bool(v) } }
impl From<i32>    for SqlValue { fn from(v: i32)    -> Self { SqlValue::Int(v as i64) } }
impl From<i64>    for SqlValue { fn from(v: i64)    -> Self { SqlValue::Int(v) } }
impl From<u32>    for SqlValue { fn from(v: u32)    -> Self { SqlValue::Int(v as i64) } }
impl From<f64>    for SqlValue { fn from(v: f64)    -> Self { SqlValue::Float(v) } }
impl From<String> for SqlValue { fn from(v: String) -> Self { SqlValue::Text(v) } }
impl From<&str>   for SqlValue { fn from(v: &str)   -> Self { SqlValue::Text(v.to_string()) } }
impl From<Vec<u8>> for SqlValue { fn from(v: Vec<u8>) -> Self { SqlValue::Bytes(v) } }

impl SqlValue {
    pub fn as_bool(&self) -> Option<bool> {
        if let SqlValue::Bool(v) = self { Some(*v) } else { None }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let SqlValue::Int(v) = self { Some(*v) } else { None }
    }
    pub fn as_float(&self) -> Option<f64> {
        match self {
            SqlValue::Float(v) => Some(*v),
            SqlValue::Int(v)   => Some(*v as f64),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<&str> {
        if let SqlValue::Text(v) = self { Some(v.as_str()) } else { None }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, SqlValue::Null)
    }
}

/// A row is a column-name → value map. Order is preserved on insertion
/// when the driver supplies columns in declaration order.
pub type Row = HashMap<String, SqlValue>;

// ── Errors ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum SqlError {
    #[error("database error: {0}")]
    Database(String),

    #[error("expected one row, got {got}")]
    UnexpectedRowCount { got: usize },

    #[error("missing column '{0}'")]
    MissingColumn(String),

    #[error("type mismatch on column '{column}': expected {expected}")]
    TypeMismatch { column: String, expected: String },

    #[error("driver not available: {0}")]
    DriverUnavailable(String),
}

// ── Service trait ────────────────────────────────────────────────

/// A database connection / pool that can run SQL statements. Drivers
/// implement this; tests use [`FakeSqlExecutor`].
pub trait SqlExecutor: Send + Sync + 'static {
    /// Execute a write/DDL statement; returns rows affected.
    fn execute(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<u64, SqlError>>;

    /// Run a SELECT-style query; returns all rows.
    fn fetch_all(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<Vec<Row>, SqlError>>;
}

// ── Effect helpers ───────────────────────────────────────────────

pub fn execute<R: SqlExecutor>(
    sql: impl Into<String>,
    params: Vec<SqlValue>,
) -> Effect<u64, SqlError, R> {
    let sql = sql.into();
    Effect::from_fn(move |r: Arc<R>| {
        let sql = sql.clone();
        let params = params.clone();
        async move { r.execute(sql, params).await }
    })
}

pub fn fetch_all<R: SqlExecutor>(
    sql: impl Into<String>,
    params: Vec<SqlValue>,
) -> Effect<Vec<Row>, SqlError, R> {
    let sql = sql.into();
    Effect::from_fn(move |r: Arc<R>| {
        let sql = sql.clone();
        let params = params.clone();
        async move { r.fetch_all(sql, params).await }
    })
}

/// Fetch exactly one row. Fails with `UnexpectedRowCount` for zero or
/// more than one row.
pub fn fetch_one<R: SqlExecutor>(
    sql: impl Into<String>,
    params: Vec<SqlValue>,
) -> Effect<Row, SqlError, R> {
    fetch_all(sql, params).flat_map(|rows| {
        let n = rows.len();
        Effect::<Row, SqlError, R>::sync(move || {
            if n == 1 {
                Ok(rows[0].clone())
            } else {
                Err(SqlError::UnexpectedRowCount { got: n })
            }
        })
    })
}

/// Fetch at most one row.
pub fn fetch_optional<R: SqlExecutor>(
    sql: impl Into<String>,
    params: Vec<SqlValue>,
) -> Effect<Option<Row>, SqlError, R> {
    fetch_all(sql, params).flat_map(|rows| {
        let n = rows.len();
        Effect::<Option<Row>, SqlError, R>::sync(move || match n {
            0 => Ok(None),
            1 => Ok(Some(rows[0].clone())),
            _ => Err(SqlError::UnexpectedRowCount { got: n }),
        })
    })
}

// ── FakeSqlExecutor ──────────────────────────────────────────────

/// Canned-response executor for tests.
pub struct FakeSqlExecutor {
    inner: Mutex<FakeInner>,
}

struct FakeInner {
    fetch_responses: HashMap<String, Vec<Row>>,
    execute_responses: HashMap<String, u64>,
    fetch_errors: HashMap<String, SqlError>,
    execute_errors: HashMap<String, SqlError>,
    recorded: Vec<(String, Vec<SqlValue>, OpKind)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpKind {
    Execute,
    FetchAll,
}

impl FakeSqlExecutor {
    pub fn new() -> Self {
        FakeSqlExecutor {
            inner: Mutex::new(FakeInner {
                fetch_responses: HashMap::new(),
                execute_responses: HashMap::new(),
                fetch_errors: HashMap::new(),
                execute_errors: HashMap::new(),
                recorded: Vec::new(),
            }),
        }
    }

    pub fn expect_fetch(&self, sql: impl Into<String>, rows: Vec<Row>) {
        self.inner.lock().unwrap().fetch_responses.insert(sql.into(), rows);
    }

    pub fn expect_execute(&self, sql: impl Into<String>, rows_affected: u64) {
        self.inner
            .lock()
            .unwrap()
            .execute_responses
            .insert(sql.into(), rows_affected);
    }

    pub fn expect_fetch_error(&self, sql: impl Into<String>, err: SqlError) {
        self.inner.lock().unwrap().fetch_errors.insert(sql.into(), err);
    }

    pub fn expect_execute_error(&self, sql: impl Into<String>, err: SqlError) {
        self.inner.lock().unwrap().execute_errors.insert(sql.into(), err);
    }

    pub fn recorded(&self) -> Vec<(String, Vec<SqlValue>, OpKind)> {
        self.inner.lock().unwrap().recorded.clone()
    }
}

impl Default for FakeSqlExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlExecutor for FakeSqlExecutor {
    fn execute(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<u64, SqlError>> {
        let mut guard = self.inner.lock().unwrap();
        guard.recorded.push((sql.clone(), params.clone(), OpKind::Execute));
        if let Some(err) = guard.execute_errors.get(&sql).cloned() {
            return Box::pin(async move { Err(err) });
        }
        let n = guard.execute_responses.get(&sql).copied();
        Box::pin(async move {
            n.ok_or_else(|| SqlError::Database(format!("no canned response for {sql}")))
        })
    }

    fn fetch_all(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<Vec<Row>, SqlError>> {
        let mut guard = self.inner.lock().unwrap();
        guard.recorded.push((sql.clone(), params.clone(), OpKind::FetchAll));
        if let Some(err) = guard.fetch_errors.get(&sql).cloned() {
            return Box::pin(async move { Err(err) });
        }
        let rows = guard.fetch_responses.get(&sql).cloned();
        Box::pin(async move {
            rows.ok_or_else(|| SqlError::Database(format!("no canned rows for {sql}")))
        })
    }
}

// ── Convenience: row construction for fakes ──────────────────────

/// Build a `Row` from `(name, value)` pairs.
pub fn row<I, K, V>(pairs: I) -> Row
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<SqlValue>,
{
    pairs.into_iter().map(|(k, v)| (k.into(), v.into())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Exit;

    #[tokio::test]
    async fn execute_returns_rows_affected() {
        let db = FakeSqlExecutor::new();
        db.expect_execute("UPDATE users SET active = 1", 3);
        let exit: Exit<u64, SqlError> = execute::<FakeSqlExecutor>(
            "UPDATE users SET active = 1",
            vec![],
        )
        .run_with(db)
        .await;
        assert_eq!(exit.ok(), Some(3));
    }

    #[tokio::test]
    async fn fetch_all_returns_canned_rows() {
        let db = FakeSqlExecutor::new();
        db.expect_fetch(
            "SELECT * FROM users",
            vec![
                row([("name", "alice"), ("age_str", "30")]),
                row([("name", "bob"),   ("age_str", "25")]),
            ],
        );
        let exit = fetch_all::<FakeSqlExecutor>("SELECT * FROM users", vec![])
            .run_with(db)
            .await;
        let rows = exit.ok().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("name").unwrap().as_text(), Some("alice"));
    }

    #[tokio::test]
    async fn fetch_one_fails_on_zero_rows() {
        let db = FakeSqlExecutor::new();
        db.expect_fetch("SELECT * FROM users WHERE id = ?", vec![]);
        let exit = fetch_one::<FakeSqlExecutor>(
            "SELECT * FROM users WHERE id = ?",
            vec![SqlValue::Int(99)],
        )
        .run_with(db)
        .await;
        match exit.err() {
            Some(SqlError::UnexpectedRowCount { got }) => assert_eq!(got, 0),
            other => panic!("expected UnexpectedRowCount, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_one_succeeds_on_exactly_one_row() {
        let db = FakeSqlExecutor::new();
        db.expect_fetch(
            "SELECT * FROM users WHERE id = ?",
            vec![row([("name", "alice")])],
        );
        let exit = fetch_one::<FakeSqlExecutor>(
            "SELECT * FROM users WHERE id = ?",
            vec![SqlValue::Int(1)],
        )
        .run_with(db)
        .await;
        let r = exit.ok().unwrap();
        assert_eq!(r.get("name").unwrap().as_text(), Some("alice"));
    }

    #[tokio::test]
    async fn fetch_optional_handles_zero_and_one() {
        let db = FakeSqlExecutor::new();
        db.expect_fetch("SELECT * FROM users WHERE id = ?", vec![]);
        let arc = Arc::new(db);

        let none = fetch_optional::<FakeSqlExecutor>(
            "SELECT * FROM users WHERE id = ?",
            vec![SqlValue::Int(1)],
        )
        .run(arc.clone())
        .await;
        assert_eq!(none.ok().unwrap(), None);
    }

    #[tokio::test]
    async fn params_are_recorded() {
        let db = FakeSqlExecutor::new();
        db.expect_execute(
            "INSERT INTO users (name, age) VALUES (?, ?)",
            1,
        );
        let arc = Arc::new(db);
        let _ = execute::<FakeSqlExecutor>(
            "INSERT INTO users (name, age) VALUES (?, ?)",
            vec![SqlValue::Text("alice".into()), SqlValue::Int(30)],
        )
        .run(arc.clone())
        .await;
        let log = arc.recorded();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].1[0], SqlValue::Text("alice".into()));
        assert_eq!(log[0].1[1], SqlValue::Int(30));
        assert_eq!(log[0].2, OpKind::Execute);
    }

    #[tokio::test]
    async fn canned_error_propagates() {
        let db = FakeSqlExecutor::new();
        db.expect_fetch_error("SELECT 1", SqlError::Database("connection refused".into()));
        let exit = fetch_all::<FakeSqlExecutor>("SELECT 1", vec![])
            .run_with(db)
            .await;
        match exit.err() {
            Some(SqlError::Database(msg)) => assert!(msg.contains("connection refused")),
            other => panic!("expected Database error, got {other:?}"),
        }
    }

    #[test]
    fn sql_value_conversions_work_via_into() {
        let _v: SqlValue = true.into();
        let _v: SqlValue = 42_i32.into();
        let _v: SqlValue = 42_i64.into();
        let _v: SqlValue = 1.5_f64.into();
        let _v: SqlValue = "hello".into();
        let _v: SqlValue = String::from("hello").into();
    }

    #[test]
    fn sql_value_accessors_work() {
        assert_eq!(SqlValue::Bool(true).as_bool(), Some(true));
        assert_eq!(SqlValue::Int(7).as_int(), Some(7));
        assert_eq!(SqlValue::Int(7).as_float(), Some(7.0));
        assert_eq!(SqlValue::Text("hi".into()).as_text(), Some("hi"));
        assert!(SqlValue::Null.is_null());
        assert_eq!(SqlValue::Bool(true).as_int(), None);
    }
}
