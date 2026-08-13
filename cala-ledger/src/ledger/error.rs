use sqlx::error::DatabaseError;
use thiserror::Error;

use crate::{
    account::error::AccountError, account_set::error::AccountSetError,
    balance::error::BalanceError, entry::error::EntryError, journal::error::JournalError,
    posting::PostingError, transaction::error::TransactionError,
    tx_template::error::TxTemplateError, velocity::error::VelocityError,
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
    EntryError(EntryError),
    #[error("LedgerError - BalanceError: {0}")]
    BalanceError(#[from] BalanceError),
    #[error("LedgerError - VelocityError: {0}")]
    VelocityError(#[from] VelocityError),
    #[error("LedgerError - PostingError: {0}")]
    PostingError(#[from] PostingError),
    #[error("LedgerError - EcRollupRegistration: {0}")]
    EcRollupRegistration(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("LedgerError - EcRollupCheckpoint: {0}")]
    EcRollupCheckpoint(obix::out::HandlerCheckpointError),
    #[error(
        "LedgerError - EcCaughtUpTimeout: EC rollup checkpoint {applied} had not reached the \
         outbox frontier {frontier} after waiting {waited:?}"
    )]
    EcCaughtUpTimeout {
        applied: obix::EventSequence,
        frontier: obix::EventSequence,
        waited: std::time::Duration,
    },
    #[error(
        "LedgerError - EntryTargetsAccountSet: an entry may not be posted directly to an \
         account-set backing account; an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet,
}

impl From<EntryError> for LedgerError {
    fn from(e: EntryError) -> Self {
        match e {
            // Surface the posting-flow domain error at the top level so
            // callers don't have to dig through the entry-error nesting.
            EntryError::EntryTargetsAccountSet => Self::EntryTargetsAccountSet,
            other => Self::EntryError(other),
        }
    }
}

/// Manual, not `#[from]`: the timeout case is remapped so callers
/// destructure ledger vocabulary (`applied`) rather than obix's
/// (`checkpoint`). Everything else passes through.
impl From<obix::out::HandlerCheckpointError> for LedgerError {
    fn from(e: obix::out::HandlerCheckpointError) -> Self {
        match e {
            obix::out::HandlerCheckpointError::CaughtUpTimeout {
                checkpoint,
                target,
                waited,
            } => Self::EcCaughtUpTimeout {
                applied: checkpoint,
                frontier: target,
                waited,
            },
            other => Self::EcRollupCheckpoint(other),
        }
    }
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
