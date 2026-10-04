use crate::primitives::*;
use es_entity::errlanes;
#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_BALANCE_NOT_FOUND")]
#[error("No balance for journal {0}, account {1}, currency {2}")]
pub struct BalanceNotFound(pub JournalId, pub AccountId, pub Currency);

#[derive(Debug, errlanes::Rejection)]
#[rejection(code = "CALA_BALANCE_ACCOUNT_LOCKED")]
#[error("Cannot update balances: account {0} is locked")]
pub struct BalanceAccountLocked(pub AccountId);
