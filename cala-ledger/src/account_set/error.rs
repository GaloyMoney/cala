use thiserror::Error;

use crate::outbox::error::OutboxError;

#[derive(Error, Debug)]
pub enum AccountSetError {
    #[error("AccountSetError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("AccountSetError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("AccountError - AccountError: {0}")]
    AccountError(#[from] crate::account::error::AccountError),
}
