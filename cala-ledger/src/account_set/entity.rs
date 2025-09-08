use derive_builder::Builder;
use es_entity::*;
use serde::{Deserialize, Serialize};

pub use cala_types::{
    account_set::*, primitives::AccountSetId, velocity::VelocityContextAccountValues,
};

use crate::primitives::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "AccountSetId", event_context = false)]
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

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct AccountSet {
    pub id: AccountSetId,
    values: AccountSetValues,
    events: EntityEvents<AccountSetEvent>,
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
        Self::try_from_events(events).expect("Failed to build account set from events")
    }

    pub fn id(&self) -> AccountSetId {
        self.values.id
    }

    pub fn values(&self) -> &AccountSetValues {
        &self.values
    }

    pub fn update(&mut self, builder: impl Into<AccountSetUpdate>) {
        let AccountSetUpdateValues {
            external_id,
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
        if external_id.is_some() && external_id != self.values().external_id {
            self.values.external_id.clone_from(&external_id);
            updated_fields.push("external_id".to_string());
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
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }
}

#[derive(Debug, Builder, Default)]
#[builder(name = "AccountSetUpdate", default)]
pub struct AccountSetUpdateValues {
    #[builder(setter(strip_option, into))]
    pub external_id: Option<String>,
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
                "external_id" => {
                    if let Some(ref ext_id) = values.external_id {
                        builder.external_id(ext_id);
                    }
                }
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

impl TryFromEvents<AccountSetEvent> for AccountSet {
    fn try_from_events(events: EntityEvents<AccountSetEvent>) -> Result<Self, EsEntityError> {
        let mut builder = AccountSetBuilder::default();
        for event in events.iter_all() {
            match event {
                #[cfg(feature = "import")]
                AccountSetEvent::Imported { source: _, values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
                AccountSetEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
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
    #[builder(setter(strip_option, into), default)]
    pub(super) external_id: Option<String>,
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

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }

    pub(super) fn context_values(&self) -> VelocityContextAccountValues {
        VelocityContextAccountValues {
            id: self.id.into(),
            name: self.name.clone(),
            normal_balance_type: self.normal_balance_type,
            external_id: self.external_id.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl IntoEvents<AccountSetEvent> for NewAccountSet {
    fn into_events(self) -> EntityEvents<AccountSetEvent> {
        EntityEvents::init(
            self.id,
            [AccountSetEvent::Initialized {
                values: AccountSetValues {
                    id: self.id,
                    version: 1,
                    journal_id: self.journal_id,
                    name: self.name,
                    external_id: self.external_id,
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
