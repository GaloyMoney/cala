use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::{entity::*, primitives::*};
pub use cala_types::{journal::*, primitives::JournalId};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: JournalValues,
    },
    Initialized {
        values: JournalValues,
    },
}

impl EntityEvent for JournalEvent {
    type EntityId = JournalId;
    fn event_table_name() -> &'static str {
        "cala_journal_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct Journal {
    values: JournalValues,
    pub(super) events: EntityEvents<JournalEvent>,
}

impl Entity for Journal {
    type Event = JournalEvent;
}

impl Journal {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: JournalValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [JournalEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from(events).expect("Failed to build account from events")
    }

    pub fn id(&self) -> JournalId {
        self.values.id
    }

    pub fn values(&self) -> &JournalValues {
        &self.values
    }

    pub fn into_values(self) -> JournalValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at
            .expect("No events for account")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .latest_event_persisted_at
            .expect("No events for account")
    }
}

impl TryFrom<EntityEvents<JournalEvent>> for Journal {
    type Error = EntityError;

    fn try_from(events: EntityEvents<JournalEvent>) -> Result<Self, Self::Error> {
        let mut builder = JournalBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                JournalEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                JournalEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

/// Representation of a new ledger journal entity
/// with required/optional properties and a builder.
#[derive(Debug, Builder)]
pub struct NewJournal {
    #[builder(setter(into))]
    pub id: JournalId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(default)]
    pub(super) status: Status,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
}

impl NewJournal {
    pub fn builder() -> NewJournalBuilder {
        NewJournalBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<JournalEvent> {
        EntityEvents::init(
            self.id,
            [JournalEvent::Initialized {
                values: JournalValues {
                    id: self.id,
                    version: 1,
                    name: self.name,
                    status: self.status,
                    description: self.description,
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
        let new_journal = NewJournal::builder()
            .id(JournalId::new())
            .name("name")
            .build()
            .unwrap();
        assert_eq!(new_journal.name, "name");
        assert_eq!(new_journal.status, Status::Active);
        assert_eq!(new_journal.description, None);
    }

    #[test]
    fn fails_when_mandatory_fields_are_missing() {
        let new_account = NewJournal::builder().build();
        assert!(new_account.is_err());
    }
}
