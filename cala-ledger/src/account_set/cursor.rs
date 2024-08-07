use cala_types::{
    account_set::{AccountSetMember, AccountSetValues},
    primitives::AccountSetId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountSetByNameCursor {
    pub name: String,
    pub id: AccountSetId,
}

impl From<&AccountSetValues> for AccountSetByNameCursor {
    fn from(values: &AccountSetValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountSetMemberCursor {
    pub member_created_at: chrono::DateTime<chrono::Utc>,
}

impl From<AccountSetMember> for AccountSetMemberCursor {
    fn from(member: AccountSetMember) -> Self {
        Self {
            member_created_at: member.created_at,
        }
    }
}
