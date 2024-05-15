use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{entry::*, primitives::EntryId};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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

impl EntityEvent for EntryEvent {
    type EntityId = EntryId;
    fn event_table_name() -> &'static str {
        "cala_entry_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct Entry {
    values: EntryValues,
    pub(super) events: EntityEvents<EntryEvent>,
}

impl Entity for Entry {
    type Event = EntryEvent;
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
        Self::try_from(events).expect("Failed to build entry from events")
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
}

impl TryFrom<EntityEvents<EntryEvent>> for Entry {
    type Error = EntityError;

    fn try_from(events: EntityEvents<EntryEvent>) -> Result<Self, Self::Error> {
        let mut builder = EntryBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                EntryEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                EntryEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
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
}

impl NewEntry {
    pub fn builder() -> NewEntryBuilder {
        NewEntryBuilder::default()
    }

    #[allow(dead_code)]
    pub(super) fn initial_events(self) -> EntityEvents<EntryEvent> {
        EntityEvents::init(
            self.id,
            [EntryEvent::Initialized {
                values: EntryValues {
                    id: self.id,
                    transaction_id: self.transaction_id,
                    journal_id: self.journal_id,
                    account_id: self.account_id,
                    entry_type: self.entry_type,
                    layer: self.layer,
                    units: self.units,
                    currency: self.currency,
                    direction: self.direction,
                    description: self.description,
                },
            }],
        )
    }
}
