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
    #[error("JournalError - code '{0}' already exists")]
    CodeAlreadyExists(String),
}

impl From<JournalCreateError> for JournalError {
    fn from(error: JournalCreateError) -> Self {
        match error {
            JournalCreateError::ConstraintViolation {
                column: Some(JournalColumn::Code),
                value,
                ..
            } => Self::CodeAlreadyExists(value.unwrap_or_default()),
            other => Self::Create(other),
        }
    }
}
