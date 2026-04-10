use thiserror::Error;

use super::repo::{
    AccountSetColumn, AccountSetCreateError, AccountSetFindError, AccountSetModifyError,
    AccountSetQueryError,
};
use crate::primitives::{AccountId, AccountSetId};

#[derive(Error, Debug)]
pub enum AccountSetError {
    #[error("AccountSetError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("AccountSetError - Create: {0}")]
    Create(AccountSetCreateError),
    #[error("AccountSetError - Modify: {0}")]
    Modify(#[from] AccountSetModifyError),
    #[error("AccountSetError - Find: {0}")]
    Find(AccountSetFindError),
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
    #[error("AccountSetError - external_id '{0}' already exists")]
    ExternalIdAlreadyExists(String),
    #[error("AccountSetError - JournalIdMismatch")]
    JournalIdMismatch,
    #[error("AccountSetError - Member already added to account set")]
    MemberAlreadyAdded,
    #[error(
        "AccountSetError - Cannot add or remove member '{member_id}' to/from \
         account set '{account_set_id}': member already has balance history \
         in this journal"
    )]
    MemberHasBalanceHistory {
        account_set_id: AccountSetId,
        member_id: AccountId,
    },
    #[error(
        "AccountSetError - Cannot recalculate account set '{account_set_id}': \
         only eventually-consistent sets support recalculation"
    )]
    CannotRecalculateNonEcSet { account_set_id: AccountSetId },
}

impl From<AccountSetFindError> for AccountSetError {
    fn from(error: AccountSetFindError) -> Self {
        match error {
            AccountSetFindError::NotFound {
                column: Some(AccountSetColumn::Id),
                value,
                ..
            } => Self::CouldNotFindById(value.parse().expect("invalid uuid")),
            AccountSetFindError::NotFound {
                column: Some(AccountSetColumn::ExternalId),
                value,
                ..
            } => Self::CouldNotFindByExternalId(value),
            other => Self::Find(other),
        }
    }
}

impl From<AccountSetCreateError> for AccountSetError {
    fn from(error: AccountSetCreateError) -> Self {
        match error {
            AccountSetCreateError::ConstraintViolation {
                column: Some(AccountSetColumn::ExternalId),
                value,
                ..
            } => Self::ExternalIdAlreadyExists(value.unwrap_or_default()),
            other => Self::Create(other),
        }
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
