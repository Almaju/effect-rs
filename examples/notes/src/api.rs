//! HTTP handlers — Schema validation in, business logic in the
//! middle, Schema encoding out.

use std::sync::Arc;

use effect::{Effect, Exit, Schema};
use effect_http_server::{Request, Response, Router, ServerError};
use effect_schema::serde_json;
use effect_sql_sqlite::SqliteExecutor;

use crate::model::*;
use crate::repo::{create_note, get_note, list_notes};

/// Build the full router, threading the database into every handler.
pub fn build_router(db: Arc<SqliteExecutor>) -> Router {
    let db_create = db.clone();
    let db_list = db.clone();
    let db_get = db;
    Router::new()
        .post("/notes/create", move |req: Request| {
            let db = db_create.clone();
            async move { handle(req, db, create).await }
        })
        .post("/notes/list", move |req: Request| {
            let db = db_list.clone();
            async move { handle(req, db, list).await }
        })
        .post("/notes/get", move |req: Request| {
            let db = db_get.clone();
            async move { handle(req, db, get).await }
        })
}

// ── Handler glue ─────────────────────────────────────────────────

/// Run a schema-typed handler: parse request body, run the effect,
/// encode response.
async fn handle<I, O, F>(
    req: Request,
    db: Arc<SqliteExecutor>,
    handler: F,
) -> Result<Response, ServerError>
where
    I: Schema + Send + 'static,
    O: Schema + Send + 'static,
    F: FnOnce(I) -> Effect<O, String, SqliteExecutor> + Send + 'static,
{
    // Log the incoming request.
    tracing::info!(target: "notes", method = ?req.method, path = %req.path, "request");

    // Parse the body as JSON, then via Schema.
    let parsed = match serde_json::from_slice::<serde_json::Value>(&req.body) {
        Ok(v) => v,
        Err(e) => {
            return Ok(Response::text(400, format!("invalid JSON: {e}")));
        }
    };
    let input = match I::parse_json(&parsed) {
        Ok(v) => v,
        Err(e) => return Ok(Response::text(400, format!("validation failed: {e}"))),
    };

    // Run the business-logic effect.
    let executor = SqliteExecutor::with_pool(db.pool().clone());
    let exit = handler(input).run_with(executor).await;

    match exit {
        Exit::Success(output) => {
            let body = serde_json::to_vec(&output.encode_json())
                .map_err(|e| ServerError::Handler(format!("encode: {e}")))?;
            Ok(Response::json(200, body))
        }
        Exit::Failure(cause) => {
            tracing::error!(target: "notes", "handler failed: {cause}");
            Ok(Response::internal_error(format!("{cause}")))
        }
    }
}

// ── Business logic per route ─────────────────────────────────────

fn create(input: CreateNote) -> Effect<Created, String, SqliteExecutor> {
    create_note(input)
        .map(|id| Created { id })
        .map_error(|e| format!("create_note failed: {e}"))
}

fn list(_: NoListInput) -> Effect<NoteList, String, SqliteExecutor> {
    list_notes()
        .map(|notes| NoteList { notes })
        .map_error(|e| format!("list_notes failed: {e}"))
}

fn get(input: GetNote) -> Effect<MaybeNote, String, SqliteExecutor> {
    get_note(input.id)
        .map(|note| MaybeNote { note })
        .map_error(|e| format!("get_note failed: {e}"))
}

/// Stub input for endpoints that take no body — Schema needs SOME
/// type, even if it's a marker.
#[derive(Debug, Clone, Schema)]
pub struct NoListInput {}

/// Stub response wrapping `Option<Note>` since Schema doesn't ship
/// container coercion for `Option<Schema>` at the JSON-root level
/// (it ships for fields).
#[derive(Debug, Clone, Schema)]
pub struct MaybeNote {
    pub note: Option<Note>,
}
