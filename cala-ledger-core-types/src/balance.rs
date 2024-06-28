use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub entry_id: EntryId,
    pub settled: BalanceAmount,
    pub pending: BalanceAmount,
    pub encumbrance: BalanceAmount,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceAmount {
    pub dr_balance: Decimal,
    pub cr_balance: Decimal,
    pub entry_id: EntryId,
    pub modified_at: DateTime<Utc>,
}
