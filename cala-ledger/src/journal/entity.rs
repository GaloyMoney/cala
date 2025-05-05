use derive_builder::Builder;
use es_entity::*;
use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{journal::*, primitives::JournalId};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "JournalId")]
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

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct Journal {
    pub id: JournalId,
    values: JournalValues,
    events: EntityEvents<JournalEvent>,
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
        Self::try_from_events(events).expect("Failed to build account from events")
    }

    pub fn id(&self) -> JournalId {
        self.values.id
    }

    pub fn values(&self) -> &JournalValues {
        &self.values
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.values.status, Status::Locked)
    }

    pub fn update(&mut self, builder: impl Into<JournalUpdate>) {
        let JournalUpdateValues {
            name,
            status,
            description,
        } = builder
            .into()
            .build()
            .expect("JournalUpdateValues always exist");
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
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }
}

#[derive(Builder, Debug, Default)]
#[builder(name = "JournalUpdate", default)]
pub struct JournalUpdateValues {
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

impl TryFromEvents<JournalEvent> for Journal {
    fn try_from_events(events: EntityEvents<JournalEvent>) -> Result<Self, EsEntityError> {
        let mut builder = JournalBuilder::default();
        for event in events.iter_all() {
            match event {
                #[cfg(feature = "import")]
                JournalEvent::Imported { source: _, values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
                JournalEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
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
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(strip_option, into), default)]
    pub(super) code: Option<String>,
    #[builder(setter(into), default)]
    status: Status,
    #[builder(setter(strip_option, into), default)]
    description: Option<String>,
    #[builder(default)]
    enable_effective_balance: bool,
}

impl NewJournal {
    pub fn builder() -> NewJournalBuilder {
        NewJournalBuilder::default()
    }

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }
}

impl IntoEvents<JournalEvent> for NewJournal {
    fn into_events(self) -> EntityEvents<JournalEvent> {
        EntityEvents::init(
            self.id,
            [JournalEvent::Initialized {
                values: JournalValues {
                    id: self.id,
                    version: 1,
                    name: self.name,
                    code: self.code,
                    status: self.status,
                    description: self.description,
                    config: JournalConfig {
                        enable_effective_balances: self.enable_effective_balance,
                    },
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
