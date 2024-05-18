use thiserror::Error;

use crate::outbox::error::OutboxError;
use cala_types::primitives::*;

#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("TransactionError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("BalanceError - OptimisticLockingError")]
    OptimisticLockingError,
    #[error("BalanceError - NotFound: there is no balance recorded for journal {0}, account {1}, currency {2}")]
    NotFound(JournalId, AccountId, Currency),
}
