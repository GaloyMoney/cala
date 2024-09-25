use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryValues {
    pub id: EntryId,
    pub version: u32,
    pub transaction_id: TransactionId,
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub entry_type: String,
    pub sequence: u32,
    pub layer: Layer,
    pub units: Decimal,
    pub currency: Currency,
    pub direction: DebitOrCredit,
    pub description: Option<String>,
}
