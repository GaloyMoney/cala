use thiserror::Error;

use crate::outbox::error::OutboxError;

#[derive(Error, Debug)]
pub enum AccountSetError {
    #[error("AccountSetEror - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("AccountSetEror - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
}
