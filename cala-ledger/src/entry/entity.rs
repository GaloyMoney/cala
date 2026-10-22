use es_entity::*;
use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{entry::*, primitives::EntryId};

/// Filter for listing a journal's entries. All bounds are inclusive and combined
/// with AND semantics; an unset bound is ignored.
#[derive(Debug, Clone, Default)]
pub struct EntriesFilter {
    /// Inclusive lower bound on the entry's creation time.
    pub created_at_from: Option<chrono::DateTime<chrono::Utc>>,
    /// Inclusive upper bound on the entry's creation time.
    pub created_at_to: Option<chrono::DateTime<chrono::Utc>>,
    /// Inclusive lower bound on the posting transaction's effective date.
    pub effective_from: Option<chrono::NaiveDate>,
    /// Inclusive upper bound on the posting transaction's effective date.
    pub effective_to: Option<chrono::NaiveDate>,
}

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "EntryId", event_context = false)]
pub enum EntryEvent {
    Initialized { values: EntryValues },
}

#[derive(EsEntity)]
pub struct Entry {
    pub id: EntryId,
    values: EntryValues,
    events: EntityEvents<EntryEvent>,
}

impl Entry {
    pub fn id(&self) -> EntryId {
        self.values.id
    }

    pub fn values(&self) -> &EntryValues {
        &self.values
    }

    pub fn into_values(self) -> EntryValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }
}

impl TryFromEvents<EntryEvent> for Entry {
    fn try_from_events(events: EntityEvents<EntryEvent>) -> Result<Self, EntityHydrationError> {
        let mut hydrator = Entry::hydrator();
        for event in events.iter_all() {
            match event {
                EntryEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

#[derive(bon::Builder, Debug)]
#[allow(dead_code)]
pub struct NewEntry {
    #[builder(into)]
    pub id: EntryId,
    #[builder(into)]
    pub(super) transaction_id: TransactionId,
    #[builder(into)]
    pub(super) journal_id: JournalId,
    #[builder(into)]
    pub(super) account_id: AccountId,
    #[builder(into)]
    pub(super) entry_type: String,
    #[builder(into)]
    pub(super) sequence: u32,
    #[builder(default)]
    pub(super) layer: Layer,
    #[builder(into)]
    pub(super) units: rust_decimal::Decimal,
    #[builder(into)]
    pub(super) currency: Currency,
    #[builder(default)]
    pub(super) direction: DebitOrCredit,
    pub(super) description: Option<String>,
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewEntry {
    /// The account this entry will be booked against. Exposed so the
    /// posting flow can take its attach-fence locks on the distinct
    /// entry accounts *before* the entry rows are inserted.
    pub(crate) fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// The entry's currency. Exposed together with [`Self::account_id`]
    /// so the posting flow can take the per-balance locks for its
    /// `(account, currency)` pairs in the same pre-insert statement.
    pub(crate) fn currency(&self) -> Currency {
        self.currency
    }
}

impl IntoEvents<EntryEvent> for NewEntry {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<EntryEvent, FromCaller>,
    ) -> EntityEvents<EntryEvent> {
        builder.initial_events(
            self.id,
            [EntryEvent::Initialized {
                values: EntryValues {
                    id: self.id,
                    version: 1,
                    transaction_id: self.transaction_id,
                    journal_id: self.journal_id,
                    account_id: self.account_id,
                    entry_type: self.entry_type,
                    sequence: self.sequence,
                    layer: self.layer,
                    units: self.units,
                    currency: self.currency,
                    direction: self.direction,
                    description: self.description,
                    metadata: self.metadata,
                },
            }],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        let currency = "USD".parse::<Currency>().unwrap();
        let entry_id = EntryId::new();
        let new_entry = NewEntry::builder()
            .id(entry_id)
            .transaction_id(TransactionId::new())
            .account_id(AccountId::new())
            .journal_id(JournalId::new())
            .layer(Layer::Settled)
            .entry_type("ENTRY_TYPE")
            .sequence(1u32)
            .units(rust_decimal::Decimal::from(1))
            .currency(currency)
            .direction(DebitOrCredit::Debit)
            .maybe_metadata(Some(serde_json::Value::String(String::new())))
            .build();
        assert_eq!(new_entry.id, entry_id);
    }
}
