use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::{entity::*, primitives::*};
pub use cala_types::{journal::*, primitives::JournalId};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEvent {
    Initialized { values: JournalValues },
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
    pub values: JournalValues,
}

impl Entity for Journal {
    type Event = JournalEvent;
}

impl TryFrom<EntityEvents<JournalEvent>> for Journal {
    type Error = EntityError;

    fn try_from(events: EntityEvents<JournalEvent>) -> Result<Self, Self::Error> {
        let mut builder = JournalBuilder::default();
        for event in events.iter() {
            match event {
                JournalEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.build()
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
    pub(super) external_id: Option<String>,
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
                    name: self.name,
                    status: self.status,
                    external_id: self.external_id,
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
