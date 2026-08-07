use thiserror::Error;

use super::repo::{
    EntryConstraint, EntryCreateError, EntryFindError, EntryModifyError, EntryQueryError,
};

#[derive(Error, Debug)]
pub enum EntryError {
    #[error("EntryError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("EntryError - Create: {0}")]
    Create(EntryCreateError),
    #[error("EntryError - Modify: {0}")]
    Modify(#[from] EntryModifyError),
    #[error("EntryError - Find: {0}")]
    Find(#[from] EntryFindError),
    #[error("EntryError - Query: {0}")]
    Query(#[from] EntryQueryError),
    #[error(
        "EntryError - EntryTargetsAccountSet: an entry may not be posted directly to an \
         account-set backing account; an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet,
}

impl From<EntryCreateError> for EntryError {
    fn from(error: EntryCreateError) -> Self {
        if error.violated_constraint() == Some(EntryConstraint::AccountNotAccountSetFkey) {
            Self::EntryTargetsAccountSet
        } else {
            Self::Create(error)
        }
    }
}
