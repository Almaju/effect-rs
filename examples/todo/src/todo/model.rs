use std::fmt;

use effect::Newtype;

/// A typed todo identifier. Distinct from any other `u64` at the type
/// level — you can't pass an arbitrary integer where a `TodoId` is
/// expected without an explicit `.into()` or `TodoId::new(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Newtype)]
pub struct TodoId(u64);

impl fmt::Display for TodoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
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
