//! Domain types for the notes service.

use effect::{Newtype, Schema};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Newtype, Schema,
)]
pub struct NoteId(i64);

#[derive(Debug, Clone, PartialEq, Schema)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub body: String,
}

/// `POST /notes/create` request shape.
#[derive(Debug, Clone, Schema)]
pub struct CreateNote {
    pub title: String,
    pub body: String,
}

/// `POST /notes/create` response shape.
#[derive(Debug, Clone, Schema)]
pub struct Created {
    pub id: NoteId,
}

/// `POST /notes/get` request shape.
#[derive(Debug, Clone, Schema)]
pub struct GetNote {
    pub id: NoteId,
}

/// `POST /notes/list` response shape.
#[derive(Debug, Clone, Schema)]
pub struct NoteList {
    pub notes: Vec<Note>,
}
