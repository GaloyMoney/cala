use sqlx::error::DatabaseError;
use thiserror::Error;

use crate::{
    account::error::AccountError, account_set::error::AccountSetError,
    balance::error::BalanceError, entry::error::EntryError, journal::error::JournalError,
    transaction::error::TransactionError, tx_template::error::TxTemplateError,
    velocity::error::VelocityError,
};

/// Composite FK forbidding a `cala_entries` row from targeting an account set.
pub(crate) const ENTRY_ACCOUNT_SET_FK: &str = "cala_entries_account_not_account_set_fkey";

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
    EntryError(#[from] EntryError),
    #[error("LedgerError - BalanceError: {0}")]
    BalanceError(#[from] BalanceError),
    #[error("LedgerError - VelocityError: {0}")]
    VelocityError(#[from] VelocityError),
    #[error("LedgerError - EcRollupRegistration: {0}")]
    EcRollupRegistration(Box<dyn std::error::Error + Send + Sync>),
    #[error(
        "LedgerError - EntryTargetsAccountSet: an entry may not be posted directly to an \
         account-set backing account; an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet,
}

impl LedgerError {
    /// Whether `err`'s source chain carries the [`ENTRY_ACCOUNT_SET_FK`] violation.
    pub(crate) fn is_entry_account_set_violation(err: &(dyn std::error::Error + 'static)) -> bool {
        let mut current = Some(err);
        while let Some(source) = current {
            if let Some(db) = source
                .downcast_ref::<sqlx::Error>()
                .and_then(|e| e.as_database_error())
            {
                if db.constraint() == Some(ENTRY_ACCOUNT_SET_FK) {
                    return true;
                }
            }
            current = source.source();
        }
        false
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
