use async_graphql::{types::connection::*, *};
use serde::{Deserialize, Serialize};

use super::{convert::ToGlobalId, primitives::*};

#[derive(InputObject)]
pub struct TransactionInput {
    pub transaction_id: UUID,
    pub tx_template_code: String,
    pub params: Option<JSON>,
}

#[derive(Clone, SimpleObject)]
pub struct Transaction {
    id: ID,
    transaction_id: UUID,
    version: u32,
    tx_template_id: UUID,
    journal_id: UUID,
    effective: Date,
    correlation_id: String,
    external_id: Option<String>,
    description: Option<String>,
    metadata: Option<JSON>,
    created_at: Timestamp,
    modified_at: Timestamp,
}

#[derive(SimpleObject)]
pub struct TransactionPostPayload {
    pub transaction: Transaction,
}

impl ToGlobalId for cala_ledger::TransactionId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "transaction:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::transaction::Transaction> for Transaction {
    fn from(entity: cala_ledger::transaction::Transaction) -> Self {
        let created_at = entity.created_at();
        let modified_at = entity.modified_at();
        let values = entity.into_values();
        Self {
            id: values.id.to_global_id(),
            transaction_id: UUID::from(values.id),
            version: values.version,
            tx_template_id: UUID::from(values.tx_template_id),
            journal_id: UUID::from(values.journal_id),
            effective: Date::from(values.effective),
            correlation_id: values.correlation_id,
            external_id: values.external_id,
            description: values.description,
            metadata: values.metadata.map(JSON::from),
            created_at: Timestamp::from(created_at),
            modified_at: Timestamp::from(modified_at),
        }
    }
}

impl From<cala_ledger::transaction::Transaction> for TransactionPostPayload {
    fn from(value: cala_ledger::transaction::Transaction) -> Self {
        Self {
            transaction: Transaction::from(value),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct TransactionByCreatedAtCursor {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub id: cala_ledger::primitives::TransactionId,
}

impl CursorType for TransactionByCreatedAtCursor {
    type Error = String;

    fn encode_cursor(&self) -> String {
        use base64::{engine::general_purpose, Engine as _};
        let json = serde_json::to_string(&self).expect("could not serialize token");
        general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
    }

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = general_purpose::STANDARD_NO_PAD
            .decode(s.as_bytes())
            .map_err(|e| e.to_string())?;
        let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}

impl From<&cala_ledger::transaction::Transaction> for TransactionByCreatedAtCursor {
    fn from(transaction: &cala_ledger::transaction::Transaction) -> Self {
        Self {
            created_at: transaction.created_at(),
            id: transaction.values().id,
        }
    }
}

impl From<TransactionByCreatedAtCursor> for cala_ledger::transaction::TransactionByCreatedAtCursor {
    fn from(cursor: TransactionByCreatedAtCursor) -> Self {
        Self {
            id: cursor.id,
            created_at: cursor.created_at,
        }
    }
}
