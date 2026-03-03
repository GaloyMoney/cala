use thiserror::Error;

use super::repo::{
    TransactionColumn, TransactionCreateError, TransactionFindError, TransactionModifyError,
    TransactionQueryError,
};
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
    Find(TransactionFindError),
    #[error("TransactionError - Query: {0}")]
    Query(#[from] TransactionQueryError),
    #[error("TransactionError - NotFound: id '{0}' not found")]
    CouldNotFindById(TransactionId),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("TransactionError - DuplicateExternalId: external_id '{0}' already exists")]
    DuplicateExternalId(String),
    #[error("TransactionError - DuplicateId: id '{0}' already exists")]
    DuplicateId(String),
    #[error("TransactionError - AlreadyVoided: transaction '{0}' is already voided")]
    AlreadyVoided(TransactionId),
}

impl TransactionError {
    pub fn was_not_found(&self) -> bool {
        matches!(
            self,
            Self::CouldNotFindById(_) | Self::CouldNotFindByExternalId(_)
        )
    }
}

impl From<TransactionFindError> for TransactionError {
    fn from(error: TransactionFindError) -> Self {
        match error {
            TransactionFindError::NotFound {
                column: Some(TransactionColumn::Id),
                value,
                ..
            } => Self::CouldNotFindById(value.parse().expect("invalid uuid")),
            TransactionFindError::NotFound {
                column: Some(TransactionColumn::ExternalId),
                value,
                ..
            } => Self::CouldNotFindByExternalId(value),
            other => Self::Find(other),
        }
    }
}

impl From<TransactionCreateError> for TransactionError {
    fn from(error: TransactionCreateError) -> Self {
        match error {
            TransactionCreateError::ConstraintViolation {
                column: Some(TransactionColumn::ExternalId),
                value,
                ..
            } => Self::DuplicateExternalId(value.unwrap_or_default()),
            TransactionCreateError::ConstraintViolation {
                column: Some(TransactionColumn::Id),
                value,
                ..
            } => Self::DuplicateId(value.unwrap_or_default()),
            other => Self::Create(other),
        }
    }
}
