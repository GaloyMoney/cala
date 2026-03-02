use thiserror::Error;

use super::repo::{
    AccountSetColumn, AccountSetCreateError, AccountSetFindError, AccountSetModifyError,
    AccountSetQueryError,
};
use crate::primitives::AccountSetId;

#[derive(Error, Debug)]
pub enum AccountSetError {
    #[error("AccountSetError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("AccountSetError - Create: {0}")]
    Create(AccountSetCreateError),
    #[error("AccountSetError - Modify: {0}")]
    Modify(#[from] AccountSetModifyError),
    #[error("AccountSetError - Find: {0}")]
    Find(#[from] AccountSetFindError),
    #[error("AccountSetError - Query: {0}")]
    Query(#[from] AccountSetQueryError),
    #[error("AccountSetError - AccountError: {0}")]
    AccountError(#[from] crate::account::error::AccountError),
    #[error("AccountSetError - BalanceError: {0}")]
    BalanceError(#[from] crate::balance::error::BalanceError),
    #[error("AccountSetError - EntryError: {0}")]
    EntryError(#[from] crate::entry::error::EntryError),
    #[error("AccountSetError - NotFound: id '{0}' not found")]
    CouldNotFindById(AccountSetId),
    #[error("AccountSetError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("AccountSetError - external_id already exists")]
    ExternalIdAlreadyExists,
    #[error("AccountSetError - JournalIdMismatch")]
    JournalIdMismatch,
    #[error("AccountSetError - Member already added to account set")]
    MemberAlreadyAdded,
}

impl From<AccountSetCreateError> for AccountSetError {
    fn from(error: AccountSetCreateError) -> Self {
        if error.was_duplicate(AccountSetColumn::ExternalId) {
            return Self::ExternalIdAlreadyExists;
        }
        Self::Create(error)
    }
}

impl From<sqlx::Error> for AccountSetError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(err) = error.as_database_error() {
            if let Some(constraint) = err.constraint() {
                if constraint
                    .contains("cala_account_set_member_accou_account_set_id_member_account_key")
                    || constraint
                        .contains("cala_account_set_member_accou_account_set_id_member_accoun_key1")
                {
                    return Self::MemberAlreadyAdded;
                }
            }
        }
        Self::Sqlx(error)
    }
}
