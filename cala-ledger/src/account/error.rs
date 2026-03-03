use thiserror::Error;

use super::repo::{
    AccountColumn, AccountCreateError, AccountFindError, AccountModifyError, AccountQueryError,
};
use crate::primitives::AccountId;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("AccountError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("AccountError - Create: {0}")]
    Create(AccountCreateError),
    #[error("AccountError - Modify: {0}")]
    Modify(#[from] AccountModifyError),
    #[error("AccountError - Find: {0}")]
    Find(AccountFindError),
    #[error("AccountError - Query: {0}")]
    Query(#[from] AccountQueryError),
    #[error("AccountError - NotFound: id '{0}' not found")]
    CouldNotFindById(AccountId),
    #[error("AccountError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("AccountError - NotFound: code '{0}' not found")]
    CouldNotFindByCode(String),
    #[error("AccountError - external_id '{0}' already exists")]
    ExternalIdAlreadyExists(String),
    #[error("AccountError - code '{0}' already exists")]
    CodeAlreadyExists(String),
    #[error("AccountError - cannot update accounts backing an AccountSet")]
    CannotUpdateAccountSetAccounts,
}

impl From<AccountFindError> for AccountError {
    fn from(error: AccountFindError) -> Self {
        match error {
            AccountFindError::NotFound {
                column: Some(AccountColumn::Id),
                value,
                ..
            } => Self::CouldNotFindById(value.parse().expect("invalid uuid")),
            AccountFindError::NotFound {
                column: Some(AccountColumn::ExternalId),
                value,
                ..
            } => Self::CouldNotFindByExternalId(value),
            AccountFindError::NotFound {
                column: Some(AccountColumn::Code),
                value,
                ..
            } => Self::CouldNotFindByCode(value),
            other => Self::Find(other),
        }
    }
}

impl From<AccountCreateError> for AccountError {
    fn from(error: AccountCreateError) -> Self {
        match error {
            AccountCreateError::ConstraintViolation {
                column: Some(AccountColumn::ExternalId),
                value,
                ..
            } => Self::ExternalIdAlreadyExists(value.unwrap_or_default()),
            AccountCreateError::ConstraintViolation {
                column: Some(AccountColumn::Code),
                value,
                ..
            } => Self::CodeAlreadyExists(value.unwrap_or_default()),
            other => Self::Create(other),
        }
    }
}
