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
            }
        }
        builder.events(events).build()
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
