use async_graphql::*;

use super::{convert::ToGlobalId, primitives::*};

#[derive(SimpleObject)]
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
