use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::{entity::*, primitives::*};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountEvent {
    Initialized { values: AccountValues },
}

impl EntityEvent for AccountEvent {
    type EntityId = AccountId;
    fn event_table_name() -> &'static str {
        "cala_account_events"
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountValues {
    pub id: AccountId,
    pub code: String,
    pub name: String,
    pub external_id: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub description: String,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
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
    #[builder(default)]
    pub(super) tags: Vec<String>,
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
                    code: self.code,
                    name: self.name,
                    external_id: self.external_id.unwrap_or_else(|| self.id.to_string()),
                    normal_balance_type: self.normal_balance_type,
                    status: self.status,
                    description: self.description.unwrap_or_default(),
                    tags: self.tags,
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
