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
    pub settled_dr_balance: Decimal,
    pub settled_cr_balance: Decimal,
    pub settled_entry_id: EntryId,
    pub settled_modified_at: DateTime<Utc>,
    pub pending_dr_balance: Decimal,
    pub pending_cr_balance: Decimal,
    pub pending_entry_id: EntryId,
    pub pending_modified_at: DateTime<Utc>,
    pub encumbered_dr_balance: Decimal,
    pub encumbered_cr_balance: Decimal,
    pub encumbered_entry_id: EntryId,
    pub encumbered_modified_at: DateTime<Utc>,
}
