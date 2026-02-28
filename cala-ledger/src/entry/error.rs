use thiserror::Error;

use super::repo::{EntryCreateError, EntryFindError, EntryModifyError, EntryQueryError};

#[derive(Error, Debug)]
pub enum EntryError {
    #[error("EntryError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("EntryError - Create: {0}")]
    Create(#[from] EntryCreateError),
    #[error("EntryError - Modify: {0}")]
    Modify(#[from] EntryModifyError),
    #[error("EntryError - Find: {0}")]
    Find(#[from] EntryFindError),
    #[error("EntryError - Query: {0}")]
    Query(#[from] EntryQueryError),
}
