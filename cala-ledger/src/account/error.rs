use thiserror::Error;

use crate::outbox::error::OutboxError;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("AccountError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("AccountError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("AccountError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
}
