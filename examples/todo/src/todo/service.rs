//! Effect-based service layer — business logic as composable effects.
//!
//! Each function is **generic over `R`** with trait bounds declaring which
//! services it needs. When you compose effects, the bounds accumulate:
//!
//! ```ignore
//! create_todo(...)   // R: TodoRepo
//! log_action(...)    // R: Logger
//! create_and_log(..) // R: TodoRepo + Logger  ← accumulated!
//! ```
//!
//! Nothing runs until you provide a context that satisfies all bounds.

use std::sync::Arc;

use effect::Effect;

use super::error::TodoError;
use super::model::{Todo, TodoId};
use super::traits::{Logger, TodoRepo};

// ── Repo operations (require TodoRepo) ───────────────────────

/// Create a new todo with title validation.
pub fn create_todo<R: TodoRepo>(title: String) -> Effect<Todo, TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| {
        let title = title.clone();
        async move {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(TodoError::InvalidTitle("cannot be empty".into()));
            }
            if trimmed.len() > 100 {
                return Err(TodoError::InvalidTitle("too long (max 100 chars)".into()));
            }
            ctx.repo().create(title)
        }
    })
}

/// Retrieve a single todo by ID.
pub fn get_todo<R: TodoRepo>(id: TodoId) -> Effect<Todo, TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| async move { ctx.repo().find_by_id(id) })
}

/// List every todo, sorted by ID.
pub fn list_todos<R: TodoRepo>() -> Effect<Vec<Todo>, TodoError, R> {
    Effect::from_fn(|ctx: Arc<R>| async move { ctx.repo().find_all() })
}

/// Mark a todo as completed.
pub fn complete_todo<R: TodoRepo>(id: TodoId) -> Effect<Todo, TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| async move { ctx.repo().update(id, None, Some(true)) })
}

/// A human-readable summary: "2/5 completed".
pub fn todo_summary<R: TodoRepo>() -> Effect<String, TodoError, R> {
    list_todos().map(|todos| {
        let total = todos.len();
        let done = todos.iter().filter(|t| t.completed).count();
        format!("{done}/{total} completed")
    })
}

// ── Logger operations (require Logger) ───────────────────────

/// Log a message through the context's logger.
pub fn log_action<R: Logger>(msg: String) -> Effect<(), TodoError, R> {
    Effect::from_fn(move |ctx: Arc<R>| {
        let msg = msg.clone();
        async move {
            ctx.log(&msg);
            Ok(())
        }
    })
}

// ── Composed operations (bounds accumulate!) ─────────────────

/// Create a todo AND log the action.
///
/// This function requires **both** `TodoRepo` and `Logger` —
/// the Rust compiler accumulates the bounds automatically,
/// just like Effect-TS accumulates the `R` union type.
pub fn create_and_log<R: TodoRepo + Logger>(title: String) -> Effect<Todo, TodoError, R> {
    create_todo(title).flat_map(|todo: Todo| {
        let msg = format!("Created: {}", todo.title);
        log_action(msg).as_value(todo)
    })
}
