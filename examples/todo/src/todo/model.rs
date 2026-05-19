use std::fmt;

pub type TodoId = u64;

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
