use thiserror::Error;

use cala_types::primitives::*;

#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("BalanceError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("BalanceError - NotFound: there is no balance recorded for journal {0}, account {1}, currency {2}")]
    NotFound(JournalId, AccountId, Currency),
    #[error("BalanceError - JournalError: {0}")]
    JournalError(#[from] crate::journal::error::JournalError),
    #[error("BalanceError - JournalLocked: Cannot update balances. The journal {0} is locked")]
    JournalLocked(JournalId),
    #[error("BalanceError - AccountLocked: Cannot update balances. The account {0} is locked")]
    AccountLocked(AccountId),
}
