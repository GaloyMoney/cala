use sqlx::error::DatabaseError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TransactionError - DuplicateKey: {0}")]
    DuplicateKey(Box<dyn DatabaseError>),
    #[error("TransactionError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
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
