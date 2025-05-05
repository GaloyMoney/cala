use sqlx::error::DatabaseError;
use thiserror::Error;

use crate::{
    account::error::AccountError, account_set::error::AccountSetError,
    balance::error::BalanceError, entry::error::EntryError, journal::error::JournalError,
    outbox::server::error::OutboxServerError, primitives::JournalId,
    transaction::error::TransactionError, tx_template::error::TxTemplateError,
    velocity::error::VelocityError,
};

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("LedgerError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("LedgerError - DuplicateKey: {0}")]
    DuplicateKey(Box<dyn DatabaseError>),
    #[error("LedgerError - Migrate: {0}")]
    SqlxMigrate(#[from] sqlx::migrate::MigrateError),
    #[error("LedgerError - Config: {0}")]
    ConfigError(String),
    #[error("LedgerError - OutboxServer: {0}")]
    OutboxServer(#[from] OutboxServerError),
    #[error("LedgerError - AccountError: {0}")]
    AccountError(#[from] AccountError),
    #[error("LedgerError - AccountSetError: {0}")]
    AccountSetError(#[from] AccountSetError),
    #[error("LedgerError - JournalError: {0}")]
    JournalError(#[from] JournalError),
    #[error("LedgerError - TxTemplateError: {0}")]
    TxTemplateError(#[from] TxTemplateError),
    #[error("LedgerError - TransactionError: {0}")]
    TransactionError(#[from] TransactionError),
    #[error("LedgerError - EntryError: {0}")]
    EntryError(#[from] EntryError),
    #[error("LedgerError - BalanceError: {0}")]
    BalanceError(#[from] BalanceError),
    #[error("LedgerError - VelocityError: {0}")]
    VelocityError(#[from] VelocityError),
}

impl From<sqlx::Error> for LedgerError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(err) if err.message().contains("duplicate key") => {
                LedgerError::DuplicateKey(err)
            }
            e => LedgerError::Sqlx(e),
        }
    }
}
