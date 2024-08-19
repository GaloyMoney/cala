use async_graphql::{dataloader::DataLoader, types::connection::*, *};
use serde::{Deserialize, Serialize};

use cala_ledger::primitives::TransactionId;

use super::{
    convert::ToGlobalId, loader::LedgerDataLoader, primitives::*, transaction::Transaction,
};

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct Entry {
    id: ID,
    entry_id: UUID,
    transaction_id: UUID,
    journal_id: UUID,
    account_id: UUID,
    entry_type: String,
    sequence: u32,
    layer: Layer,
    units: Decimal,
    currency: CurrencyCode,
    direction: DebitOrCredit,
    description: Option<String>,
    created_at: Timestamp,
    modified_at: Timestamp,
}

#[ComplexObject]
impl Entry {
    async fn transaction(&self, ctx: &Context<'_>) -> async_graphql::Result<Transaction> {
        let transaction_id = TransactionId::from(self.transaction_id);
        let ctx = ctx.data_unchecked::<DataLoader<LedgerDataLoader>>();
        let transaction = ctx
            .load_one(transaction_id)
            .await?
            .expect("a transaction should always exist for an entry");
        Ok(transaction)
    }
}

impl ToGlobalId for cala_ledger::EntryId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "entry:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_ledger::entry::Entry> for Entry {
    fn from(entity: cala_ledger::entry::Entry) -> Self {
        let created_at = entity.created_at();
        let modified_at = entity.modified_at();
        let values = entity.into_values();
        Self {
            id: values.id.to_global_id(),
            entry_id: UUID::from(values.id),
            transaction_id: UUID::from(values.transaction_id),
            journal_id: UUID::from(values.journal_id),
            account_id: UUID::from(values.account_id),
            entry_type: values.entry_type,
            sequence: values.sequence,
            layer: values.layer.into(),
            units: values.units.into(),
            currency: values.currency.into(),
            direction: values.direction.into(),
            description: values.description,
            created_at: Timestamp::from(created_at),
            modified_at: Timestamp::from(modified_at),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct EntryByCreatedAtCursor {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub sequence: u32,
    pub id: cala_ledger::primitives::EntryId,
}

impl CursorType for EntryByCreatedAtCursor {
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

impl From<&cala_ledger::entry::Entry> for EntryByCreatedAtCursor {
    fn from(entry: &cala_ledger::entry::Entry) -> Self {
        Self {
            created_at: entry.created_at(),
            id: entry.values().id,
            sequence: entry.values().sequence,
        }
    }
}

impl From<EntryByCreatedAtCursor> for cala_ledger::entry::EntryByCreatedAtCursor {
    fn from(cursor: EntryByCreatedAtCursor) -> Self {
        Self {
            id: cursor.id,
            created_at: cursor.created_at,
            sequence: cursor.sequence,
        }
    }
}
