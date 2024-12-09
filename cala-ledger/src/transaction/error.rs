use thiserror::Error;

use cala_types::primitives::TransactionId;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("TransactionError - NotFound: id '{0}' not found")]
    CouldNotFindById(TransactionId),
    #[error("TransactionError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("TransactionError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("TransactionError - DuplicateExternalId: external_id already exists")]
    DuplicateExternalId,
    #[error("TransactionError - DuplicateId: id already exists")]
    DuplicateId,
}

impl From<sqlx::Error> for TransactionError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(ref err) if err.is_unique_violation() => {
                let Some(constraint) = err.constraint() else {
                    return Self::Sqlx(e);
                };
                if constraint.contains("external_id") {
                    Self::DuplicateExternalId
                } else if constraint.contains("id") {
                    Self::DuplicateId
                } else {
                    Self::Sqlx(e)
                }
            }
            e => Self::Sqlx(e),
        }
    }
}

es_entity::from_es_entity_error!(TransactionError);
