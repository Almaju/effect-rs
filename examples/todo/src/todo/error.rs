use super::model::TodoId;
use std::fmt;

#[derive(Debug, Clone)]
pub enum TodoError {
    NotFound(TodoId),
    InvalidTitle(String),
}

impl fmt::Display for TodoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TodoError::NotFound(id) => write!(f, "Todo not found: {id}"),
            TodoError::InvalidTitle(reason) => write!(f, "Invalid title: {reason}"),
        }
    }
}
