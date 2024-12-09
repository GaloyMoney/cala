use sqlx::error::DatabaseError;
use thiserror::Error;

use cala_types::primitives::TransactionId;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TransactionError - DuplicateKey: {0}")]
    DuplicateKey(Box<dyn DatabaseError>),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("TransactionError - NotFound: id '{0}' not found")]
    CouldNotFindById(TransactionId),
    #[error("TransactionError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("TransactionError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("TransactionError - external_id already exists")]
    ExternalIdAlreadyExists,
    #[error("TransactionError - correlation_id already exists")]
    CorrelationIdAlreadyExists,
}

impl From<sqlx::Error> for TransactionError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(err) = error.as_database_error() {
            if let Some(constraint) = err.constraint() {
                if constraint.contains("external_id") {
                    return Self::ExternalIdAlreadyExists;
                } else if constraint.contains("correlation_id") {
                    return Self::CorrelationIdAlreadyExists;
                }
            }
        }
        Self::Sqlx(error)
    }
}

es_entity::from_es_entity_error!(TransactionError);
