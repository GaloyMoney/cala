use serde::{Deserialize, Serialize};

use super::{account_set::AccountSetValues, primitives::*};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountValues {
    pub id: AccountId,
    pub version: u32,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub config: AccountConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountValuesForContext {
    pub id: AccountId,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub external_id: String,
    pub metadata: Option<serde_json::Value>,
}

impl From<&AccountValues> for AccountValuesForContext {
    fn from(values: &AccountValues) -> Self {
        Self {
            id: values.id,
            name: values.name.clone(),
            normal_balance_type: values.normal_balance_type,
            external_id: values.external_id.clone().unwrap_or(values.id.to_string()),
            metadata: values.metadata.clone(),
        }
    }
}

impl From<&AccountSetValues> for AccountValuesForContext {
    fn from(values: &AccountSetValues) -> Self {
        Self {
            id: values.id.into(),
            name: values.name.clone(),
            normal_balance_type: values.normal_balance_type,
            external_id: values.external_id.clone().unwrap_or(values.id.to_string()),
            metadata: values.metadata.clone(),
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub is_account_set: bool,
    pub eventually_consistent: bool,
}

mod cel {
    use cel_interpreter::{CelMap, CelValue};

    impl From<super::AccountValuesForContext> for CelValue {
        fn from(account: super::AccountValuesForContext) -> Self {
            let mut map = CelMap::new();
            map.insert("id", account.id);
            map.insert("name", account.name.clone());
            map.insert("externalId", account.external_id.clone());
            map.insert("normalBalanceType", account.normal_balance_type);
            if let Some(metadata) = &account.metadata {
                map.insert("metadata", metadata.clone());
            }
            map.into()
        }
    }
}
