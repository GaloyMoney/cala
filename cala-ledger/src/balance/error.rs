use thiserror::Error;

use cala_types::primitives::*;

#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("BalanceError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("BalanceError - NotFound: there is no balance recorded for journal {0}, account {1}, currency {2}")]
    NotFound(JournalId, AccountId, Currency),
}
