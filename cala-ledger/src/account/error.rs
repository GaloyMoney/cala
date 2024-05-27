use thiserror::Error;

use crate::primitives::AccountId;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("AccountError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("AccountError - EntityError: {0}")]
    EntityError(#[from] crate::entity::EntityError),
    #[error("AccountError - NotFound: id '{0}' not found")]
    CouldNotFindById(AccountId),
    #[error("AccountError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("AccountError - external_id already exists")]
    ExternalIdAlreadyExists,
    #[error("AccountError - code already exists")]
    CodeAlreadyExists,
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(err) = error.as_database_error() {
            if let Some(constraint) = err.constraint() {
                if constraint.contains("external_id") {
                    return Self::ExternalIdAlreadyExists;
                } else if constraint.contains("code") {
                    return Self::CodeAlreadyExists;
                }
            }
        }
        Self::Sqlx(error)
    }
}
