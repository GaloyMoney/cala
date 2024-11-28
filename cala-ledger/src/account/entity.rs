use derive_builder::Builder;
use es_entity::*;
use serde::{Deserialize, Serialize};

pub use cala_types::{account::*, primitives::AccountId};

use crate::primitives::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "AccountId")]
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

#[derive(EsEntity, Builder)]
#[builder(pattern = "owned", build_fn(error = "EsEntityError"))]
pub struct Account {
    pub id: AccountId,
    values: AccountValues,
    pub(super) events: EntityEvents<AccountEvent>,
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
        Self::try_from_events(events).expect("Failed to build account from events")
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

    pub fn update(&mut self, builder: impl Into<AccountUpdate>) {
        let AccountUpdateValues {
            external_id,
            code,
            name,
            normal_balance_type,
            description,
            status,
            metadata,
        } = builder
            .into()
            .build()
            .expect("AccountUpdateValues always exist");

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
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }

    pub fn metadata<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        match &self.values.metadata {
            Some(metadata) => Ok(Some(serde_json::from_value(metadata.clone())?)),
            None => Ok(None),
        }
    }
}

impl TryFromEvents<AccountEvent> for Account {
    fn try_from_events(events: EntityEvents<AccountEvent>) -> Result<Self, EsEntityError> {
        let mut builder = AccountBuilder::default();
        for event in events.iter_all() {
            match event {
                #[cfg(feature = "import")]
                AccountEvent::Imported { source: _, values } => {
                    builder = builder.id(values.id).values(values.clone());
                }
                AccountEvent::Initialized { values } => {
                    builder = builder.id(values.id).values(values.clone());
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
    #[builder(setter(strip_option, into))]
    pub external_id: Option<String>,
    #[builder(setter(strip_option, into))]
    pub code: Option<String>,
    #[builder(setter(strip_option, into))]
    pub name: Option<String>,
    #[builder(setter(strip_option, into))]
    pub normal_balance_type: Option<DebitOrCredit>,
    #[builder(setter(strip_option, into))]
    pub description: Option<String>,
    #[builder(setter(strip_option, into))]
    pub status: Option<Status>,
    #[builder(setter(custom))]
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

impl From<(AccountValues, Vec<String>)> for AccountUpdate {
    fn from((values, fields): (AccountValues, Vec<String>)) -> Self {
        let mut builder = AccountUpdate::default();
        for field in fields {
            match field.as_str() {
                "external_id" => {
                    if let Some(ref ext_id) = values.external_id {
                        builder.external_id(ext_id);
                    }
                }
                "code" => {
                    builder.code(values.code.clone());
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
                "status" => {
                    builder.status(values.status);
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

/// Representation of a ***new*** ledger account entity with required/optional properties and a builder.
#[derive(Builder, Debug, Clone)]
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

    pub(super) fn data_source(&self) -> DataSource {
        DataSource::Local
    }

    pub(super) fn into_values(self) -> AccountValues {
        AccountValues {
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
        }
    }
}

impl IntoEvents<AccountEvent> for NewAccount {
    fn into_events(self) -> EntityEvents<AccountEvent> {
        let values = self.into_values();
        EntityEvents::init(values.id, [AccountEvent::Initialized { values }])
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
