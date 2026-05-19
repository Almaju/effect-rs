use std::collections::HashMap;
use std::sync::Mutex;

use super::error::TodoError;
use super::model::{Todo, TodoId};

/// In-memory todo repository — a simple HashMap behind a Mutex.
pub struct InMemoryTodoRepo {
    todos: Mutex<HashMap<TodoId, Todo>>,
    next_id: Mutex<TodoId>,
}

impl InMemoryTodoRepo {
    pub fn new() -> Self {
        Self {
            todos: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }

    pub fn find_all(&self) -> Result<Vec<Todo>, TodoError> {
        let todos = self.todos.lock().unwrap();
        let mut list: Vec<Todo> = todos.values().cloned().collect();
        list.sort_by_key(|t| t.id);
        Ok(list)
    }

    pub fn find_by_id(&self, id: TodoId) -> Result<Todo, TodoError> {
        let todos = self.todos.lock().unwrap();
        todos.get(&id).cloned().ok_or(TodoError::NotFound(id))
    }

    pub fn create(&self, title: String) -> Result<Todo, TodoError> {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;

        let todo = Todo {
            id,
            title,
            completed: false,
        };

        self.todos.lock().unwrap().insert(id, todo.clone());
        Ok(todo)
    }

    pub fn update(
        &self,
        id: TodoId,
        title: Option<String>,
        completed: Option<bool>,
    ) -> Result<Todo, TodoError> {
        let mut todos = self.todos.lock().unwrap();
        let todo = todos.get_mut(&id).ok_or(TodoError::NotFound(id))?;

        if let Some(t) = title {
            todo.title = t;
        }
        if let Some(c) = completed {
            todo.completed = c;
        }

        Ok(todo.clone())
    }

    pub fn delete(&self, id: TodoId) -> Result<(), TodoError> {
        let mut todos = self.todos.lock().unwrap();
        todos.remove(&id).ok_or(TodoError::NotFound(id))?;
        Ok(())
    }
}
