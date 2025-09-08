use derive_builder::Builder;
use es_entity::*;
use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{entry::*, primitives::EntryId};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "EntryId", event_context = false)]
pub enum EntryEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: EntryValues,
    },
    Initialized {
        values: EntryValues,
    },
}

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct Entry {
    pub id: EntryId,
    values: EntryValues,
    events: EntityEvents<EntryEvent>,
}

impl Entry {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: EntryValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [EntryEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from_events(events).expect("Failed to build entry from events")
    }

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
    fn try_from_events(events: EntityEvents<EntryEvent>) -> Result<Self, EsEntityError> {
        let mut builder = EntryBuilder::default();
        for event in events.iter_all() {
            match event {
                #[cfg(feature = "import")]
                EntryEvent::Imported { source: _, values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
                EntryEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Builder, Debug)]
#[allow(dead_code)]
pub struct NewEntry {
    #[builder(setter(into))]
    pub id: EntryId,
    #[builder(setter(into))]
    pub(super) transaction_id: TransactionId,
    #[builder(setter(into))]
    pub(super) journal_id: JournalId,
    #[builder(setter(into))]
    pub(super) account_id: AccountId,
    #[builder(setter(into))]
    pub(super) entry_type: String,
    #[builder(setter(into))]
    pub(super) sequence: u32,
    #[builder(default)]
    pub(super) layer: Layer,
    #[builder(setter(into))]
    pub(super) units: rust_decimal::Decimal,
    #[builder(setter(into))]
    pub(super) currency: Currency,
    #[builder(default)]
    pub(super) direction: DebitOrCredit,
    #[builder(setter(strip_option), default)]
    pub(super) description: Option<String>,
    #[builder(setter(into), default)]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewEntry {
    pub fn builder() -> NewEntryBuilder {
        NewEntryBuilder::default()
    }

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }
}

impl IntoEvents<EntryEvent> for NewEntry {
    fn into_events(self) -> EntityEvents<EntryEvent> {
        EntityEvents::init(
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
        let mut builder = NewEntry::builder();
        let currency = "USD".parse::<Currency>().unwrap();
        let entry_id = EntryId::new();
        builder
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
            .metadata(Some(serde_json::Value::String(String::new())));
        let new_entry = builder.build().unwrap();
        assert_eq!(new_entry.id, entry_id);
    }

    #[test]
    fn fails_when_missing_required_fields() {
        let mut builder = NewEntry::builder();
        builder.id(EntryId::new());
        let new_entry = builder.build();
        assert!(new_entry.is_err());
    }
}
