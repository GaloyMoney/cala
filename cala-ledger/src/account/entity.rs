use derive_builder::Builder;
use serde::{Deserialize, Serialize};

pub use cala_types::{account::*, primitives::AccountId};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountEvent {
    #[cfg(feature = "import")]
    Imported {
        source: DataSource,
        values: AccountValues,
    },
    Initialized {
        values: AccountValues,
    },
    Updated {
        values: AccountValues,
        fields: Vec<String>,
    },
}

impl EntityEvent for AccountEvent {
    type EntityId = AccountId;
    fn event_table_name() -> &'static str {
        "cala_account_events"
    }
}

#[derive(Builder)]
#[builder(pattern = "owned", build_fn(error = "EntityError"))]
pub struct Account {
    values: AccountValues,
    pub(super) events: EntityEvents<AccountEvent>,
}

impl Entity for Account {
    type Event = AccountEvent;
}

impl Account {
    #[cfg(feature = "import")]
    pub(super) fn import(source: DataSourceId, values: AccountValues) -> Self {
        let events = EntityEvents::init(
            values.id,
            [AccountEvent::Imported {
                source: DataSource::Remote { id: source },
                values,
            }],
        );
        Self::try_from(events).expect("Failed to build account from events")
    }

    pub fn id(&self) -> AccountId {
        self.values.id
    }

    pub fn values(&self) -> &AccountValues {
        &self.values
    }

    pub fn into_values(self) -> AccountValues {
        self.values
    }

    pub fn update(&mut self, builder: AccountUpdate) {
        let AccountUpdateValues {
            external_id,
            code,
            name,
            normal_balance_type,
            description,
            status,
            metadata,
        } = builder.build().expect("AccountUpdateValues always exist");

        let mut updated_fields = Vec::new();

        if let Some(code) = code {
            if code != self.values().code {
                self.values.code.clone_from(&code);
                updated_fields.push("code".to_string());
            }
        }
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
        if let Some(status) = status {
            if status != self.values().status {
                self.values.status.clone_from(&status);
                updated_fields.push("status".to_string());
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
            self.events.push(AccountEvent::Updated {
                values: self.values.clone(),
                fields: updated_fields,
            });
        }
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

    pub fn metadata<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        match &self.values.metadata {
            Some(metadata) => Ok(Some(serde_json::from_value(metadata.clone())?)),
            None => Ok(None),
        }
    }
}

impl TryFrom<EntityEvents<AccountEvent>> for Account {
    type Error = EntityError;

    fn try_from(events: EntityEvents<AccountEvent>) -> Result<Self, Self::Error> {
        let mut builder = AccountBuilder::default();
        for event in events.iter() {
            match event {
                #[cfg(feature = "import")]
                AccountEvent::Imported { source: _, values } => {
                    builder = builder.values(values.clone());
                }
                AccountEvent::Initialized { values } => {
                    builder = builder.values(values.clone());
                }
                AccountEvent::Updated { values, .. } => {
                    builder = builder.values(values.clone());
                }
            }
        }
        builder.events(events).build()
    }
}

#[derive(Debug, Builder, Default)]
#[builder(name = "AccountUpdate", default)]
pub struct AccountUpdateValues {
    pub external_id: Option<String>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub normal_balance_type: Option<DebitOrCredit>,
    pub description: Option<String>,
    pub status: Option<Status>,
    #[builder(setter(custom), default)]
    pub metadata: Option<serde_json::Value>,
}

impl AccountUpdate {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }
}

/// Representation of a ***new*** ledger account entity with required/optional properties and a builder.
#[derive(Builder, Debug)]
pub struct NewAccount {
    #[builder(setter(into))]
    pub id: AccountId,
    #[builder(setter(into))]
    pub(super) code: String,
    #[builder(setter(into))]
    pub(super) name: String,
    #[builder(setter(strip_option, into), default)]
    pub(super) external_id: Option<String>,
    #[builder(default)]
    pub(super) normal_balance_type: DebitOrCredit,
    #[builder(setter(strip_option, into), default)]
    pub(super) description: Option<String>,
    #[builder(default)]
    pub(super) status: Status,
    #[builder(setter(custom), default)]
    pub(super) metadata: Option<serde_json::Value>,
    #[builder(setter(custom), default)]
    pub(super) is_account_set: bool,
    #[builder(setter(custom), default)]
    pub(super) eventually_consistent: bool,
}

impl NewAccount {
    pub fn builder() -> NewAccountBuilder {
        NewAccountBuilder::default()
    }

    pub(super) fn initial_events(self) -> EntityEvents<AccountEvent> {
        EntityEvents::init(
            self.id,
            [AccountEvent::Initialized {
                values: AccountValues {
                    id: self.id,
                    version: 1,
                    code: self.code,
                    name: self.name,
                    external_id: self.external_id,
                    normal_balance_type: self.normal_balance_type,
                    status: self.status,
                    description: self.description,
                    metadata: self.metadata,
                    config: AccountConfig {
                        is_account_set: self.is_account_set,
                        eventually_consistent: false,
                    },
                },
            }],
        )
    }
}

impl NewAccountBuilder {
    pub fn metadata<T: serde::Serialize>(
        &mut self,
        metadata: T,
    ) -> Result<&mut Self, serde_json::Error> {
        self.metadata = Some(Some(serde_json::to_value(metadata)?));
        Ok(self)
    }

    pub(crate) fn is_account_set(&mut self, is_account_set: bool) -> &mut Self {
        self.is_account_set = Some(is_account_set);
        self.eventually_consistent(is_account_set)
    }

    fn eventually_consistent(&mut self, eventually_consistent: bool) -> &mut Self {
        self.is_account_set = Some(eventually_consistent);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::new_v4())
            .code("code")
            .name("name")
            .build()
            .unwrap();
        assert_eq!(new_account.code, "code");
        assert_eq!(new_account.name, "name");
        assert_eq!(new_account.normal_balance_type, DebitOrCredit::Credit);
        assert_eq!(new_account.description, None);
        assert_eq!(new_account.status, Status::Active);
        assert_eq!(new_account.metadata, None);
    }

    #[test]
    fn fails_when_mandatory_fields_are_missing() {
        let new_account = NewAccount::builder().build();
        assert!(new_account.is_err());
    }

    #[test]
    fn accepts_metadata() {
        use serde_json::json;
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::new_v4())
            .code("code")
            .name("name")
            .metadata(json!({"foo": "bar"}))
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(new_account.metadata, Some(json!({"foo": "bar"})));
    }
}
