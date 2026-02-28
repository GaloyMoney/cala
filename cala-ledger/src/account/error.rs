use thiserror::Error;

use super::repo::{AccountColumn, AccountCreateError, AccountFindError, AccountModifyError, AccountQueryError};
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
    Find(#[from] AccountFindError),
    #[error("AccountError - Query: {0}")]
    Query(#[from] AccountQueryError),
    #[error("AccountError - NotFound: id '{0}' not found")]
    CouldNotFindById(AccountId),
    #[error("AccountError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("AccountError - NotFound: code '{0}' not found")]
    CouldNotFindByCode(String),
    #[error("AccountError - external_id already exists")]
    ExternalIdAlreadyExists,
    #[error("AccountError - code already exists")]
    CodeAlreadyExists,
    #[error("AccountError - cannot update accounts backing an AccountSet")]
    CannotUpdateAccountSetAccounts,
}

impl From<AccountCreateError> for AccountError {
    fn from(error: AccountCreateError) -> Self {
        if error.was_duplicate(AccountColumn::ExternalId) {
            return Self::ExternalIdAlreadyExists;
        }
        if error.was_duplicate(AccountColumn::Code) {
            return Self::CodeAlreadyExists;
        }
        Self::Create(error)
    }
}
