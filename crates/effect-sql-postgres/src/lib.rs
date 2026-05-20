//! `sqlx::PgPool` driver for [`effect_sql`].
//!
//! Pattern mirrors `effect-sql-sqlite`:
//!
//! ```no_run
//! use effect_sql::{execute, SqlValue};
//! use effect_sql_postgres::PostgresExecutor;
//!
//! # #[tokio::main] async fn main() {
//! let db = PostgresExecutor::connect("postgres://user:pass@localhost/dbname")
//!     .await
//!     .expect("connect");
//!
//! let _ = execute::<PostgresExecutor>(
//!     "INSERT INTO users (name) VALUES ($1)",
//!     vec![SqlValue::Text("alice".into())],
//! ).run_with(db).await;
//! # }
//! ```
//!
//! **Tests:** real Postgres integration tests aren't included in this
//! crate (would require a live database). The driver code is small;
//! validate end-to-end in your own test suite against a dev DB.

use effect_sql::{AsyncResult, Row, SqlError, SqlExecutor, SqlValue};
use sqlx::postgres::{PgPool, PgRow};
use sqlx::{Column, Row as _, TypeInfo, ValueRef};

pub struct PostgresExecutor {
    pool: PgPool,
}

impl PostgresExecutor {
    /// Connect to a Postgres database at the given URL.
    pub async fn connect(url: &str) -> Result<Self, SqlError> {
        let pool = PgPool::connect(url)
            .await
            .map_err(|e| SqlError::Database(format!("connect: {e}")))?;
        Ok(PostgresExecutor { pool })
    }

    /// Wrap an existing `PgPool`.
    pub fn with_pool(pool: PgPool) -> Self {
        PostgresExecutor { pool }
    }

    /// Borrow the underlying pool — for migrations, transactions, or
    /// any driver-specific code.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl SqlExecutor for PostgresExecutor {
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
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: SqlValue,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match value {
        SqlValue::Null => query.bind(Option::<i64>::None),
        SqlValue::Bool(b) => query.bind(b),
        SqlValue::Int(n) => query.bind(n),
        SqlValue::Float(f) => query.bind(f),
        SqlValue::Text(s) => query.bind(s),
        SqlValue::Bytes(b) => query.bind(b),
    }
}

fn decode_row(row: PgRow) -> Result<Row, SqlError> {
    let mut out = Row::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let val = decode_column(&row, i, &name)?;
        out.insert(name, val);
    }
    Ok(out)
}

/// Postgres types are statically typed per-column, so we look at
/// `type_info().name()` and decode accordingly.
fn decode_column(row: &PgRow, idx: usize, name: &str) -> Result<SqlValue, SqlError> {
    let raw = row.try_get_raw(idx).map_err(|e| {
        SqlError::Database(format!("raw read failed for column '{name}': {e}"))
    })?;
    if raw.is_null() {
        return Ok(SqlValue::Null);
    }
    let type_name = raw.type_info().name().to_string();

    // Common Pg type names → SqlValue.
    // Numeric types
    match type_name.as_str() {
        "BOOL" => return row
            .try_get::<bool, _>(idx)
            .map(SqlValue::Bool)
            .map_err(|e| type_err(name, "BOOL", e)),
        "INT2" => return row
            .try_get::<i16, _>(idx)
            .map(|v| SqlValue::Int(v as i64))
            .map_err(|e| type_err(name, "INT2", e)),
        "INT4" => return row
            .try_get::<i32, _>(idx)
            .map(|v| SqlValue::Int(v as i64))
            .map_err(|e| type_err(name, "INT4", e)),
        "INT8" => return row
            .try_get::<i64, _>(idx)
            .map(SqlValue::Int)
            .map_err(|e| type_err(name, "INT8", e)),
        "FLOAT4" => return row
            .try_get::<f32, _>(idx)
            .map(|v| SqlValue::Float(v as f64))
            .map_err(|e| type_err(name, "FLOAT4", e)),
        "FLOAT8" => return row
            .try_get::<f64, _>(idx)
            .map(SqlValue::Float)
            .map_err(|e| type_err(name, "FLOAT8", e)),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "CHAR" => return row
            .try_get::<String, _>(idx)
            .map(SqlValue::Text)
            .map_err(|e| type_err(name, &type_name, e)),
        "BYTEA" => return row
            .try_get::<Vec<u8>, _>(idx)
            .map(SqlValue::Bytes)
            .map_err(|e| type_err(name, "BYTEA", e)),
        _ => {}
    }

    // Fallback: try common types in order.
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
        expected: format!(
            "supported SqlValue mapping for Postgres type '{type_name}'"
        ),
    })
}

fn type_err(column: &str, pg_type: &str, e: sqlx::Error) -> SqlError {
    SqlError::TypeMismatch {
        column: column.to_string(),
        expected: format!("{pg_type} decoded successfully (sqlx: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connection failures should propagate as SqlError::Database. We
    /// can run this without a live Postgres — connecting to an obviously
    /// unreachable URL fails predictably.
    #[tokio::test]
    async fn connect_to_invalid_url_returns_database_error() {
        let result = PostgresExecutor::connect("postgres://x:y@127.0.0.1:1/no_such_db").await;
        match result {
            Err(SqlError::Database(_)) => {}
            Err(other) => panic!("expected Database error, got {other:?}"),
            Ok(_) => panic!("expected failure, got connection"),
        }
    }
}
