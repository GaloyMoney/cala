use es_entity::errlanes;

use cala_types::primitives::*;

#[derive(errlanes::Rejection, Debug)]
pub enum BalanceRejection {
    #[error("BalanceRejection - NotFound: there is no balance recorded for journal {0}, account {1}, currency {2}")]
    #[rejection(code = "CALA_BALANCE_NOT_FOUND")]
    NotFound(JournalId, AccountId, Currency),
    #[error("BalanceRejection - AccountLocked: Cannot update balances. The account {0} is locked")]
    #[rejection(code = "CALA_BALANCE_ACCOUNT_LOCKED")]
    AccountLocked(AccountId),
}
