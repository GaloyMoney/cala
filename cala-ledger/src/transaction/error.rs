use thiserror::Error;

use super::repo::{TransactionColumn, TransactionCreateError, TransactionFindError, TransactionModifyError, TransactionQueryError};
use cala_types::primitives::TransactionId;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("TransactionError - Create: {0}")]
    Create(TransactionCreateError),
    #[error("TransactionError - Modify: {0}")]
    Modify(#[from] TransactionModifyError),
    #[error("TransactionError - Find: {0}")]
    Find(#[from] TransactionFindError),
    #[error("TransactionError - Query: {0}")]
    Query(#[from] TransactionQueryError),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("TransactionError - NotFound: id '{0}' not found")]
    CouldNotFindById(TransactionId),
    #[error("TransactionError - DuplicateExternalId: external_id '{0}' already exists")]
    DuplicateExternalId(String),
    #[error("TransactionError - DuplicateId: id '{0}' already exists")]
    DuplicateId(String),
    #[error("TransactionError - AlreadyVoided: transaction '{0}' is already voided")]
    AlreadyVoided(TransactionId),
}

impl TransactionError {
    pub fn was_not_found(&self) -> bool {
        matches!(self, Self::Find(e) if e.was_not_found())
    }
}

impl From<TransactionCreateError> for TransactionError {
    fn from(error: TransactionCreateError) -> Self {
        if let Some(value) = error.duplicate_value() {
            if error.was_duplicate(TransactionColumn::ExternalId) {
                return Self::DuplicateExternalId(value.to_string());
            }
            if error.was_duplicate(TransactionColumn::Id) {
                return Self::DuplicateId(value.to_string());
            }
        }
        Self::Create(error)
    }
}
