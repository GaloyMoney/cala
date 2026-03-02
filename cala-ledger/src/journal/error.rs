use thiserror::Error;

use super::repo::{
    JournalColumn, JournalCreateError, JournalFindError, JournalModifyError, JournalQueryError,
};

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("JournalError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JournalError - Create: {0}")]
    Create(JournalCreateError),
    #[error("JournalError - Modify: {0}")]
    Modify(#[from] JournalModifyError),
    #[error("JournalError - Find: {0}")]
    Find(#[from] JournalFindError),
    #[error("JournalError - Query: {0}")]
    Query(#[from] JournalQueryError),
    #[error("JournalError - code already exists")]
    CodeAlreadyExists,
}

impl From<JournalCreateError> for JournalError {
    fn from(error: JournalCreateError) -> Self {
        if error.was_duplicate(JournalColumn::Code) {
            return Self::CodeAlreadyExists;
        }
        Self::Create(error)
    }
}
