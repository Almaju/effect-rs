//! Service traits — the Rust equivalent of Effect-TS "Tags".
//!
//! Each trait declares that a context **provides** a particular service.
//! When used as a bound on `R`, it means the effect **requires** that service.
//!
//! ```ignore
//! // This effect requires a repo:
//! fn get_todo<R: HasRepo>(id: u64) -> Effect<Todo, TodoError, R>
//!
//! // This one requires repo + logger — bounds accumulate:
//! fn create_and_log<R: HasRepo + HasLogger>(t: String) -> Effect<Todo, TodoError, R>
//! ```

use super::repo::InMemoryTodoRepo;

/// Declares that a context provides access to a todo repository.
pub trait HasRepo: Send + Sync + 'static {
    fn repo(&self) -> &InMemoryTodoRepo;
}

/// Declares that a context provides logging capabilities.
pub trait HasLogger: Send + Sync + 'static {
    fn log(&self, msg: &str);
}
