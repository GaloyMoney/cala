use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{account_set::*, primitives::AccountSetId};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountSetEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: AccountSetValues,
    },
    Initialized {
        values: AccountSetValues,
    },
    Updated {
        values: AccountSetValues,
        fields: Vec<String>,
    },
}

impl EntityEvent for AccountSetEvent {
    type EntityId = AccountSetId;
    fn event_table_name() -> &'static str {
        "cala_account_set_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct AccountSet {
    values: AccountSetValues,
    pub(super) events: EntityEvents<AccountSetEvent>,
}

impl Entity for AccountSet {
    type Event = AccountSetEvent;
}

impl AccountSet {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: AccountSetValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [AccountSetEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from(events).expect("Failed to build account set from events")
    }

    pub fn id(&self) -> AccountSetId {
        self.values.id
    }

    pub fn values(&self) -> &AccountSetValues {
        &self.values
    }

    pub fn update(&mut self, builder: impl Into<AccountSetUpdate>) {
        let AccountSetUpdateValues {
            name,
            normal_balance_type,
            description,
            metadata,
        } = builder
            .into()
            .build()
            .expect("AccountSetUpdateValues always exist");
        let mut updated_fields = Vec::new();

        if let Some(name) = name {
            if name != self.values().name {
                self.values.name.clone_from(&name);
                updated_fields.push("name".to_string());
            }
        }
        if let Some(normal_balance_type) = normal_balance_type {
            if normal_balance_type != self.values().normal_balance_type {
                self.values
                    .normal_balance_type
                    .clone_from(&normal_balance_type);
                updated_fields.push("normal_balance_type".to_string());
            }
        }
        if description.is_some() && description != self.values().description {
            self.values.description.clone_from(&description);
            updated_fields.push("description".to_string());
        }
        if let Some(metadata) = metadata {
            if metadata != serde_json::Value::Null
                && Some(&metadata) != self.values().metadata.as_ref()
            {
                self.values.metadata = Some(metadata);
                updated_fields.push("metadata".to_string());
            }
        }

        if !updated_fields.is_empty() {
            self.events.push(AccountSetEvent::Updated {
                values: self.values.clone(),
                fields: updated_fields,
            });
        }
    }

    pub fn into_values(self) -> AccountSetValues {
        self.values
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at
            .expect("No events for account set")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .latest_event_persisted_at
            .expect("No events for account set")
    }
}

#[derive(Debug, Builder, Default)]
#[builder(name = "AccountSetUpdate", default)]
pub struct AccountSetUpdateValues {
    #[builder(setter(into, strip_option))]
    pub name: Option<String>,
    #[builder(setter(into, strip_option))]
    pub normal_balance_type: Option<DebitOrCredit>,
    #[builder(setter(into, strip_option))]
    pub description: Option<String>,
    #[builder(setter(custom))]
    pub metadata: Option<serde_json::Value>,
}

impl AccountSetUpdate {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }
}

impl From<(AccountSetValues, Vec<String>)> for AccountSetUpdate {
    fn from((values, fields): (AccountSetValues, Vec<String>)) -> Self {
        let mut builder = AccountSetUpdate::default();

        for field in fields {
            match field.as_str() {
                "name" => {
                    builder.name(values.name.clone());
                }

                "normal_balance_type" => {
                    builder.normal_balance_type(values.normal_balance_type);
                }

                "description" => {
                    if let Some(ref desc) = values.description {
                        builder.description(desc);
                    }
                }

                "metadata" => {
                    if let Some(metadata) = values.metadata.clone() {
                        builder
                            .metadata(metadata)
                            .expect("Failed to serialize metadata");
                    }
                }
                _ => unreachable!("Unknown field: {}", field),
            }
        }
        builder
    }
}

impl TryFrom<EntityEvents<AccountSetEvent>> for AccountSet {
    type Error = EntityError;

    fn try_from(events: EntityEvents<AccountSetEvent>) -> Result<Self, Self::Error> {
        let mut builder = AccountSetBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                AccountSetEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                AccountSetEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
                AccountSetEvent::Updated { values, .. } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

/// Representation of a ***new*** ledger account set entity with required/optional properties and a builder.
#[derive(Builder, Debug)]
pub struct NewAccountSet {
    #[builder(setter(into))]
    pub id: AccountSetId,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(into))]
    pub(super) journal_id: JournalId,
    #[builder(default)]
    pub(super) normal_balance_type: DebitOrCredit,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(setter(custom), default)]
    pub(super) metadata: Option<serde_json::Value>,
}

impl NewAccountSet {
    pub fn builder() -> NewAccountSetBuilder {
        NewAccountSetBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<AccountSetEvent> {
        EntityEvents::init(
            self.id,
            [AccountSetEvent::Initialized {
                values: AccountSetValues {
                    id: self.id,
                    version: 1,
                    journal_id: self.journal_id,
                    name: self.name,
                    normal_balance_type: self.normal_balance_type,
                    description: self.description,
                    metadata: self.metadata,
                },
            }],
        )
    }
}

impl NewAccountSetBuilder {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }
}
