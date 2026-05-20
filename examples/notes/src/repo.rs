//! Sqlite-backed note storage.

use std::sync::Arc;

use effect::Effect;
use effect_sql::{execute, fetch_all, fetch_optional, Row, SqlError, SqlValue};
use effect_sql_sqlite::SqliteExecutor;

use crate::model::{CreateNote, Note, NoteId};

/// Ensure the `notes` table exists. `id` is `INTEGER PRIMARY KEY`
/// (rowid alias) and is supplied by the caller — see [`next_id`].
pub async fn init_schema(db: &SqliteExecutor) -> Result<(), SqlError> {
    let exit = execute::<SqliteExecutor>(
        "CREATE TABLE IF NOT EXISTS notes (\n             id INTEGER PRIMARY KEY,\n             title TEXT NOT NULL,\n             body TEXT NOT NULL\n         )",
        vec![],
    )
    .run_with(db.clone_pool_wrapper())
    .await;
    match exit.into_result() {
        Ok(_) => Ok(()),
        Err(c) => match c.into_failure() {
            Some(e) => Err(e),
            None => Err(SqlError::Database("init schema failed".into())),
        },
    }
}

/// Generate a fresh id from the current nanosecond-precision clock
/// plus a process-local counter (so bursts produce unique values).
///
/// Side-steps the "fetch DB-assigned id" pool-state quirk while
/// keeping the example small. A real app would use UUIDs or
/// Snowflake-style ids.
fn next_id() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicI64 = AtomicI64::new(0);
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64;
    base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Insert a note with a client-generated id; return that id.
pub fn create_note(input: CreateNote) -> Effect<NoteId, SqlError, SqliteExecutor> {
    let title = input.title;
    let body = input.body;
    let id = next_id();
    Effect::block(move |g| {
        let title = title.clone();
        let body = body.clone();
        async move {
            g.run(execute::<SqliteExecutor>(
                "INSERT INTO notes (id, title, body) VALUES (?, ?, ?)",
                vec![
                    SqlValue::Int(id),
                    SqlValue::Text(title),
                    SqlValue::Text(body),
                ],
            ))
            .await?;
            Ok(NoteId::new(id))
        }
    })
}

pub fn list_notes() -> Effect<Vec<Note>, SqlError, SqliteExecutor> {
    fetch_all::<SqliteExecutor>("SELECT id, title, body FROM notes ORDER BY id", vec![])
        .flat_map(|rows| {
            let parsed: Result<Vec<Note>, SqlError> = rows.into_iter().map(row_to_note).collect();
            Effect::<Vec<Note>, SqlError, SqliteExecutor>::sync(move || parsed.clone())
        })
}

pub fn get_note(id: NoteId) -> Effect<Option<Note>, SqlError, SqliteExecutor> {
    fetch_optional::<SqliteExecutor>(
        "SELECT id, title, body FROM notes WHERE id = ?",
        vec![SqlValue::Int(id.into_inner())],
    )
    .flat_map(|maybe_row| {
        let result: Result<Option<Note>, SqlError> = match maybe_row {
            Some(row) => row_to_note(row).map(Some),
            None => Ok(None),
        };
        Effect::<Option<Note>, SqlError, SqliteExecutor>::sync(move || result.clone())
    })
}

fn row_to_note(row: Row) -> Result<Note, SqlError> {
    let id = row
        .get("id")
        .and_then(SqlValue::as_int)
        .ok_or_else(|| SqlError::MissingColumn("id".into()))?;
    let title = row
        .get("title")
        .and_then(|v| v.as_text().map(String::from))
        .ok_or_else(|| SqlError::MissingColumn("title".into()))?;
    let body = row
        .get("body")
        .and_then(|v| v.as_text().map(String::from))
        .ok_or_else(|| SqlError::MissingColumn("body".into()))?;
    Ok(Note {
        id: NoteId::new(id),
        title,
        body,
    })
}

/// Clone the underlying SqlitePool to get a fresh `SqliteExecutor`.
/// The pool is Arc-shared so this is cheap and connection-safe.
pub trait CloneExecutor {
    fn clone_pool_wrapper(&self) -> SqliteExecutor;
}

impl CloneExecutor for SqliteExecutor {
    fn clone_pool_wrapper(&self) -> SqliteExecutor {
        SqliteExecutor::with_pool(self.pool().clone())
    }
}

/// Build a thread-safe `Arc<SqliteExecutor>` for sharing across
/// handlers.
///
/// We deliberately limit the pool to a single connection so that
/// `:memory:` databases work correctly (each sqlite connection
/// otherwise gets its own private memory db). SQLite is single-writer
/// in any case — bumping pool size rarely pays off here.
pub async fn open_db(path: &str) -> Result<Arc<SqliteExecutor>, SqlError> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(path)
        .await
        .map_err(|e| SqlError::Database(format!("connect: {e}")))?;
    let db = SqliteExecutor::with_pool(pool);
    init_schema(&db).await?;
    Ok(Arc::new(db))
}
