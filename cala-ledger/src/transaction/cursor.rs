use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use cala_types::primitives::TransactionId;

use super::Transaction;

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionByCreatedAtCursor {
    pub created_at: DateTime<Utc>,
    pub id: TransactionId,
}

impl From<&Transaction> for TransactionByCreatedAtCursor {
    fn from(transaction: &Transaction) -> Self {
        Self {
            created_at: transaction.created_at(),
            id: transaction.values().id,
        }
    }
}
