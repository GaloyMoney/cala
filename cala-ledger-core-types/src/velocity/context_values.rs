use serde::{Deserialize, Serialize};

use crate::{account::AccountValues, account_set::AccountSetValues, primitives::*};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VelocityContextAccountValues {
    pub id: AccountId,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub external_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl From<&AccountValues> for VelocityContextAccountValues {
    fn from(values: &AccountValues) -> Self {
        Self {
            id: values.id,
            name: values.name.clone(),
            normal_balance_type: values.normal_balance_type,
            external_id: values.external_id.clone(),
            metadata: values.metadata.clone(),
        }
    }
}

impl From<AccountValues> for VelocityContextAccountValues {
    fn from(values: AccountValues) -> Self {
        Self {
            id: values.id,
            name: values.name,
            normal_balance_type: values.normal_balance_type,
            external_id: values.external_id,
            metadata: values.metadata,
        }
    }
}

impl From<&AccountSetValues> for VelocityContextAccountValues {
    fn from(values: &AccountSetValues) -> Self {
        Self {
            id: values.id.into(),
            name: values.name.clone(),
            normal_balance_type: values.normal_balance_type,
            external_id: values.external_id.clone(),
            metadata: values.metadata.clone(),
        }
    }
}

mod cel {
    use cel_interpreter::{CelMap, CelValue};

    impl From<&super::VelocityContextAccountValues> for CelValue {
        fn from(account: &super::VelocityContextAccountValues) -> Self {
            let mut map = CelMap::new();
            map.insert("id", account.id);
            map.insert("name", account.name.clone());
            map.insert(
                "externalId",
                account.external_id.clone().unwrap_or_default(),
            );
            map.insert("normalBalanceType", account.normal_balance_type);
            if let Some(metadata) = &account.metadata {
                map.insert("metadata", metadata.clone());
            }
            map.into()
        }
    }
}
mod sqlx {
    use sqlx::{
        postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef},
        Postgres,
    };

    use super::VelocityContextAccountValues;

    impl sqlx::Type<Postgres> for VelocityContextAccountValues {
        fn type_info() -> PgTypeInfo {
            <serde_json::Value as sqlx::Type<Postgres>>::type_info()
        }
    }

    impl<'q> sqlx::Encode<'q, Postgres> for VelocityContextAccountValues {
        fn encode_by_ref(
            &self,
            buf: &mut PgArgumentBuffer,
        ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            let json_value = serde_json::to_value(self)?;
            <serde_json::Value as sqlx::Encode<Postgres>>::encode_by_ref(&json_value, buf)
        }
    }

    impl<'r> sqlx::Decode<'r, Postgres> for VelocityContextAccountValues {
        fn decode(
            value: PgValueRef<'r>,
        ) -> Result<Self, Box<dyn std::error::Error + 'static + Send + Sync>> {
            let json_value = <serde_json::Value as sqlx::Decode<Postgres>>::decode(value)?;
            let res: VelocityContextAccountValues = serde_json::from_value(json_value)?;
            Ok(res)
        }
    }

    impl PgHasArrayType for VelocityContextAccountValues {
        fn array_type_info() -> PgTypeInfo {
            <serde_json::Value as sqlx::postgres::PgHasArrayType>::array_type_info()
        }
    }
}
