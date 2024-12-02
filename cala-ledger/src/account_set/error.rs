use thiserror::Error;

use crate::primitives::AccountSetId;

#[derive(Error, Debug)]
pub enum AccountSetError {
    #[error("AccountSetError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("AccountSetError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("AccountSetError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("AccountSetError - AccountError: {0}")]
    AccountError(#[from] crate::account::error::AccountError),
    #[error("AccountSetError - BalanceError: {0}")]
    BalanceError(#[from] crate::balance::error::BalanceError),
    #[error("AccountSetError - EntryError: {0}")]
    EntryError(#[from] crate::entry::error::EntryError),
    #[error("AccountSetError - NotFound: id '{0}' not found")]
    CouldNotFindById(AccountSetId),
    #[error("AccountSetError - JournalIdMismatch")]
    JournalIdMismatch,
    #[error("AccountSetError - EntryError: {0}")]
    EntryError(#[from] crate::entry::error::EntryError),
}

es_entity::from_es_entity_error!(AccountSetError);
