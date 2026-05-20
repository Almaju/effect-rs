//! `sqlx::SqlitePool` driver for [`effect_sql`].
//!
//! ```no_run
//! use effect_sql::{execute, fetch_all, SqlValue};
//! use effect_sql_sqlite::SqliteExecutor;
//!
//! # #[tokio::main] async fn main() {
//! let db = SqliteExecutor::connect("sqlite::memory:")
//!     .await
//!     .expect("connect");
//!
//! let _ = execute::<SqliteExecutor>(
//!     "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
//!     vec![],
//! ).run_with(db).await;
//! # }
//! ```

use effect_sql::{AsyncResult, Row, SqlError, SqlExecutor, SqlValue};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Column, Row as _, TypeInfo, ValueRef};

pub struct SqliteExecutor {
    pool: SqlitePool,
}

impl SqliteExecutor {
    /// Open or create the SQLite database at `url`. For an in-memory
    /// DB, use `"sqlite::memory:"`.
    pub async fn connect(url: &str) -> Result<Self, SqlError> {
        let pool = SqlitePool::connect(url)
            .await
            .map_err(|e| SqlError::Database(format!("connect: {e}")))?;
        Ok(SqliteExecutor { pool })
    }

    /// Wrap an existing `SqlitePool`.
    pub fn with_pool(pool: SqlitePool) -> Self {
        SqliteExecutor { pool }
    }

    /// Borrow the underlying pool — useful for migrations or
    /// driver-specific queries.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl SqlExecutor for SqliteExecutor {
    fn execute(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<u64, SqlError>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let mut q = sqlx::query(&sql);
            for p in params {
                q = bind(q, p);
            }
            let result = q
                .execute(&pool)
                .await
                .map_err(|e| SqlError::Database(e.to_string()))?;
            Ok(result.rows_affected())
        })
    }

    fn fetch_all(
        &self,
        sql: String,
        params: Vec<SqlValue>,
    ) -> AsyncResult<Result<Vec<Row>, SqlError>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let mut q = sqlx::query(&sql);
            for p in params {
                q = bind(q, p);
            }
            let rows = q
                .fetch_all(&pool)
                .await
                .map_err(|e| SqlError::Database(e.to_string()))?;
            rows.into_iter().map(decode_row).collect()
        })
    }
}

fn bind<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: SqlValue,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match value {
        SqlValue::Null => query.bind(Option::<i64>::None),
        SqlValue::Bool(b) => query.bind(b),
        SqlValue::Int(n) => query.bind(n),
        SqlValue::Float(f) => query.bind(f),
        SqlValue::Text(s) => query.bind(s),
        SqlValue::Bytes(b) => query.bind(b),
    }
}

fn decode_row(row: SqliteRow) -> Result<Row, SqlError> {
    let mut out = Row::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let val = decode_column(&row, i, &name)?;
        out.insert(name, val);
    }
    Ok(out)
}

fn decode_column(row: &SqliteRow, idx: usize, name: &str) -> Result<SqlValue, SqlError> {
    let raw = row.try_get_raw(idx).map_err(|e| {
        SqlError::Database(format!("raw read failed for column '{name}': {e}"))
    })?;
    if raw.is_null() {
        return Ok(SqlValue::Null);
    }
    let type_name = raw.type_info().name().to_string();
    // SQLite stores values dynamically — try common Rust types in order.
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Ok(SqlValue::Int(v));
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return Ok(SqlValue::Float(v));
    }
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return Ok(SqlValue::Text(v));
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return Ok(SqlValue::Bytes(v));
    }
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return Ok(SqlValue::Bool(v));
    }
    Err(SqlError::TypeMismatch {
        column: name.to_string(),
        expected: format!("decodable from sqlite type {type_name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect_sql::{execute, fetch_all, fetch_one, fetch_optional};
    use std::sync::Arc;

    async fn fresh_db() -> Arc<SqliteExecutor> {
        let db = SqliteExecutor::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        Arc::new(db)
    }

    #[tokio::test]
    async fn create_insert_select_roundtrips() {
        let db = fresh_db().await;

        let _ = execute::<SqliteExecutor>(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL)",
            vec![],
        )
        .run(db.clone())
        .await;

        let inserted = execute::<SqliteExecutor>(
            "INSERT INTO users (name, score) VALUES (?, ?)",
            vec![SqlValue::Text("alice".into()), SqlValue::Float(9.5)],
        )
        .run(db.clone())
        .await
        .ok()
        .unwrap();
        assert_eq!(inserted, 1);

        let rows = fetch_all::<SqliteExecutor>("SELECT name, score FROM users", vec![])
            .run(db.clone())
            .await
            .ok()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("name").unwrap().as_text(), Some("alice"));
        assert_eq!(rows[0].get("score").unwrap().as_float(), Some(9.5));
    }

    #[tokio::test]
    async fn fetch_one_succeeds_and_fails_appropriately() {
        let db = fresh_db().await;
        let _ = execute::<SqliteExecutor>(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER)",
            vec![],
        )
        .run(db.clone())
        .await;
        let _ = execute::<SqliteExecutor>("INSERT INTO t (v) VALUES (?)", vec![SqlValue::Int(7)])
            .run(db.clone())
            .await;

        let row = fetch_one::<SqliteExecutor>("SELECT v FROM t", vec![])
            .run(db.clone())
            .await
            .ok()
            .unwrap();
        assert_eq!(row.get("v").unwrap().as_int(), Some(7));

        // Two rows → UnexpectedRowCount
        let _ = execute::<SqliteExecutor>("INSERT INTO t (v) VALUES (?)", vec![SqlValue::Int(8)])
            .run(db.clone())
            .await;
        let err = fetch_one::<SqliteExecutor>("SELECT v FROM t", vec![])
            .run(db.clone())
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SqlError::UnexpectedRowCount { got: 2 }));
    }

    #[tokio::test]
    async fn fetch_optional_returns_none_when_no_match() {
        let db = fresh_db().await;
        let _ = execute::<SqliteExecutor>("CREATE TABLE u (id INTEGER PRIMARY KEY)", vec![])
            .run(db.clone())
            .await;
        let result = fetch_optional::<SqliteExecutor>(
            "SELECT * FROM u WHERE id = ?",
            vec![SqlValue::Int(999)],
        )
        .run(db.clone())
        .await
        .ok()
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn null_round_trips_as_sql_value_null() {
        let db = fresh_db().await;
        let _ = execute::<SqliteExecutor>("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)", vec![])
            .run(db.clone())
            .await;
        let _ = execute::<SqliteExecutor>("INSERT INTO t (v) VALUES (NULL)", vec![])
            .run(db.clone())
            .await;
        let row = fetch_one::<SqliteExecutor>("SELECT v FROM t", vec![])
            .run(db.clone())
            .await
            .ok()
            .unwrap();
        assert!(row.get("v").unwrap().is_null());
    }

    #[tokio::test]
    async fn invalid_sql_surfaces_database_error() {
        let db = fresh_db().await;
        let err = execute::<SqliteExecutor>("THIS IS NOT SQL", vec![])
            .run(db.clone())
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SqlError::Database(_)));
    }
}
