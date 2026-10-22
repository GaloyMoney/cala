use es_entity::*;
use serde::{Deserialize, Serialize};

use crate::primitives::*;
pub use cala_types::{journal::*, primitives::JournalId};

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "JournalId", event_context = false)]
pub enum JournalEvent {
    Initialized {
        values: JournalValues,
    },
    Updated {
        values: JournalValues,
        fields: Vec<String>,
    },
}

#[derive(EsEntity)]
pub struct Journal {
    pub id: JournalId,
    values: JournalValues,
    events: EntityEvents<JournalEvent>,
}

impl Journal {
    pub fn id(&self) -> JournalId {
        self.values.id
    }

    pub fn values(&self) -> &JournalValues {
        &self.values
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.values.status, Status::Locked)
    }

    pub(crate) fn insert_effective_balances(&self) -> bool {
        self.values.config.enable_effective_balances
    }

    pub fn update(&mut self, update: impl Into<JournalUpdate>) -> es_entity::Idempotent<()> {
        let JournalUpdate {
            name,
            status,
            description,
        } = update.into();
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

        if updated_fields.is_empty() {
            return es_entity::Idempotent::AlreadyApplied;
        } else {
            self.events.push(JournalEvent::Updated {
                values: self.values.clone(),
                fields: updated_fields,
            });
        }

        es_entity::Idempotent::Executed(())
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

#[derive(Debug, Default, bon::Builder)]
pub struct JournalUpdate {
    #[builder(into)]
    pub name: Option<String>,
    #[builder(into)]
    pub status: Option<Status>,
    #[builder(into)]
    pub description: Option<String>,
}

impl From<(JournalValues, Vec<String>)> for JournalUpdate {
    fn from((values, fields): (JournalValues, Vec<String>)) -> Self {
        let mut update = JournalUpdate::default();

        for field in fields {
            match field.as_str() {
                "name" => {
                    update.name = Some(values.name.clone());
                }
                "status" => {
                    update.status = Some(values.status);
                }
                "description" => {
                    if let Some(ref desc) = values.description {
                        update.description = Some(desc.clone());
                    }
                }
                _ => unreachable!("Unknown field: {}", field),
            }
        }
        update
    }
}

impl TryFromEvents<JournalEvent> for Journal {
    fn try_from_events(events: EntityEvents<JournalEvent>) -> Result<Self, EntityHydrationError> {
        let mut hydrator = Journal::hydrator();
        for event in events.iter_all() {
            match event {
                JournalEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
                JournalEvent::Updated { values, .. } => {
                    hydrator.values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

/// Representation of a new ledger journal entity
/// with required/optional properties and a builder.
#[derive(Debug, bon::Builder)]
pub struct NewJournal {
    #[builder(into)]
    pub id: JournalId,
    #[builder(into)]
    pub(super) name: String,
    #[builder(into)]
    pub(super) code: Option<String>,
    #[builder(into, default)]
    status: Status,
    #[builder(into)]
    description: Option<String>,
    #[builder(default)]
    enable_effective_balance: bool,
}

impl IntoEvents<JournalEvent> for NewJournal {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<JournalEvent, FromCaller>,
    ) -> EntityEvents<JournalEvent> {
        builder.initial_events(
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
            .build();
        assert_eq!(new_journal.name, "name");
        assert_eq!(new_journal.status, Status::Active);
        assert_eq!(new_journal.description, None);
    }
}
