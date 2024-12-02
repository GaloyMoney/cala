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
}

impl From<sqlx::Error> for TransactionError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(err) if err.message().contains("duplicate key") => {
                Self::DuplicateKey(err)
            }
            e => Self::Sqlx(e),
        }
    }
}

es_entity::from_es_entity_error!(TransactionError);
