use super::{account::*, primitives::*};

trait ToGlobalId {
    fn to_global_id(&self) -> async_graphql::types::ID;
}

impl From<AccountByNameCursor> for cala_types::query::AccountByNameCursor {
    fn from(cursor: AccountByNameCursor) -> Self {
        Self {
            name: cursor.name,
            id: cursor.id,
        }
    }
}

impl ToGlobalId for cala_types::primitives::AccountId {
    fn to_global_id(&self) -> async_graphql::types::ID {
        use base64::{engine::general_purpose, Engine as _};
        let id = format!(
            "account:{}",
            general_purpose::STANDARD_NO_PAD.encode(self.to_string())
        );
        async_graphql::types::ID::from(id)
    }
}

impl From<cala_types::account::AccountValues> for Account {
    fn from(values: cala_types::account::AccountValues) -> Self {
        Self {
            id: values.id.to_global_id(),
            account_id: UUID::from(values.id),
            code: values.code,
            name: values.name,
            normal_balance_type: DebitOrCredit::from(values.normal_balance_type),
            status: Status::from(values.status),
            external_id: values.external_id,
            description: values.description,
            tags: values.tags.into_iter().map(TAG::from).collect(),
            metadata: values.metadata.map(JSON::from),
        }
    }
}

impl From<&cala_types::account::AccountValues> for AccountByNameCursor {
    fn from(values: &cala_types::account::AccountValues) -> Self {
        Self {
            name: values.name.clone(),
            id: values.id,
        }
    }
}
