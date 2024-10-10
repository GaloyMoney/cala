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
    Updated {
        values: JournalValues,
        fields: Vec<String>,
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

    pub fn update(&mut self, builder: impl Into<JournalUpdate>) {
        let mut builder = builder.into();
        if builder.modified_at.is_none() {
            builder.modified_at(chrono::Utc::now());
        }

        let JournalUpdateValues {
            name,
            modified_at,
            status,
            description,
        } = builder.build().expect("JournalUpdateValues always exist");
        let mut updated_fields = Vec::new();

        if let Some(name) = name {
            if name != self.values().name {
                self.values.name.clone_from(&name);
                updated_fields.push("name".to_string());
            }
        }
        if let Some(status) = status {
            if status != self.values().status {
                self.values.status.clone_from(&status);
                updated_fields.push("status".to_string());
            }
        }
        if description.is_some() && description != self.values().description {
            self.values.description.clone_from(&description);
            updated_fields.push("description".to_string());
        }

        self.values.modified_at = modified_at;

        if !updated_fields.is_empty() {
            self.events.push(JournalEvent::Updated {
                values: self.values.clone(),
                fields: updated_fields,
            });
        }
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

#[derive(Builder, Debug, Default)]
#[builder(name = "JournalUpdate", default)]
pub struct JournalUpdateValues {
    #[builder(private)]
    modified_at: chrono::DateTime<chrono::Utc>,
    #[builder(setter(into, strip_option))]
    pub name: Option<String>,
    #[builder(setter(into, strip_option))]
    pub status: Option<Status>,
    #[builder(setter(into, strip_option))]
    pub description: Option<String>,
}

impl From<(JournalValues, Vec<String>)> for JournalUpdate {
    fn from((values, fields): (JournalValues, Vec<String>)) -> Self {
        let mut builder = JournalUpdate::default();
        builder.modified_at(values.modified_at);

        for field in fields {
            match field.as_str() {
                "name" => {
                    builder.name(values.name.clone());
                }
                "status" => {
                    builder.status(values.status);
                }
                "description" => {
                    if let Some(ref desc) = values.description {
                        builder.description(desc);
                    }
                }
                _ => unreachable!("Unknown field: {}", field),
            }
        }
        builder
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
                JournalEvent::Updated { values, .. } => {
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
    #[builder(private)]
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into), default)]
    pub(super) status: Status,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
}

impl NewJournal {
    pub fn builder() -> NewJournalBuilder {
        let mut builder = NewJournalBuilder::default();
        builder.created_at(chrono::Utc::now());
        builder
    }

    pub(super) fn initial_events(self) -> EntityEvents<JournalEvent> {
        EntityEvents::init(
            self.id,
            [JournalEvent::Initialized {
                values: JournalValues {
                    id: self.id,
                    version: 1,
                    created_at: self.created_at,
                    modified_at: self.created_at,
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
