use thiserror::Error;

use crate::primitives::JournalId;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("JournalError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("JournalError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("UserError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("UserError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("AccountError - NotFound: id '{0}' not found")]
    CouldNotFindById(JournalId),
}

es_entity::from_es_entity_error!(JournalError);
