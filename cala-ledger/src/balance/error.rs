use thiserror::Error;

use crate::outbox::error::OutboxError;

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
}
