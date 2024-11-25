use serde::{Deserialize, Serialize};

use super::primitives::*;

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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub is_account_set: bool,
    pub eventually_consistent: bool,
}

mod cel {
    use cel_interpreter::{CelMap, CelValue};

    impl From<&super::AccountValues> for CelValue {
        fn from(account: &super::AccountValues) -> Self {
            let mut map = CelMap::new();
            map.insert("id", account.id);
            map.insert("code", account.code.clone());
            map.insert("name", account.name.clone());
            map.insert("externalId", account.code.clone());
            map.insert("normalBalanceType", account.normal_balance_type);
            if let Some(metadata) = &account.metadata {
                map.insert("metadata", metadata.clone());
            }
            map.into()
        }
    }
}
