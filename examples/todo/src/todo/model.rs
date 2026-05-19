use std::fmt;

use effect::{Newtype, Schema};

/// A typed todo identifier. Distinct from any other `u64` at the type
/// level — you can't pass an arbitrary integer where a `TodoId` is
/// expected without an explicit `.into()` or `TodoId::new(...)`.
///
/// `#[derive(Schema)]` on a single-field tuple struct emits a
/// transparent Schema — `TodoId` parses and encodes as a plain u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Newtype, Schema)]
pub struct TodoId(u64);

impl fmt::Display for TodoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Schema)]
pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub completed: bool,
}

impl fmt::Display for Todo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.completed { "✅" } else { "⬜" };
        write!(f, "{status} [{}] {}", self.id, self.title)
    }
}
