use cala_types::{account::AccountValues, primitives::AccountId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountByNameCursor {
    pub name: String,
    pub id: AccountId,
}

impl From<&AccountValues> for AccountByNameCursor {
    fn from(values: &AccountValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}
