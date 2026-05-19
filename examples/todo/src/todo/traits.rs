//! Service traits — the Rust equivalent of Effect-TS "Tags".
//!
//! Each trait declares that a context **provides** a particular service.
//! When used as a bound on `R`, it means the effect **requires** that service.
//!
//! ```ignore
//! // This effect requires a repo:
//! fn get_todo<R: TodoRepo>(id: u64) -> Effect<Todo, TodoError, R>
//!
//! // This one requires repo + logger — bounds accumulate:
//! fn create_and_log<R: TodoRepo + Logger>(t: String) -> Effect<Todo, TodoError, R>
//! ```

use super::repo::InMemoryTodoRepo;

/// The todo repository service.
pub trait TodoRepo: Send + Sync + 'static {
    fn repo(&self) -> &InMemoryTodoRepo;
}

/// The logger service.
pub trait Logger: Send + Sync + 'static {
    fn log(&self, msg: &str);
}
