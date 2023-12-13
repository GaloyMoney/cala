use crate::outbox::error::OutboxError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("JournalError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("JournalError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
}
