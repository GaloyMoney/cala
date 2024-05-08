use thiserror::Error;

use crate::outbox::error::OutboxError;

#[derive(Error, Debug)]
pub enum TxTemplateError {
    #[error("TxTemplateError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    OutboxError(#[from] OutboxError),
    #[error("TxTemplateError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
}
