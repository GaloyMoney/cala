use es_entity::*;
use sqlx::PgPool;

use crate::primitives::{DataSourceId, DebitOrCredit};

use super::{entity::*, error::AccountError};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Account",
    err = "AccountError",
    columns(
        name(ty = "String", update(accessor = "values().name"), list_by),
        code(ty = "String", update(accessor = "values().code"), list_by),
        external_id(
            ty = "Option<String>",
            update(accessor = "values().external_id"),
            list_by
        ),
        normal_balance_type(
            ty = "DebitOrCredit",
            update(accessor = "values().normal_balance_type")
        ),
        eventually_consistent(ty = "bool", update(persist = false)),
        latest_values(
            ty = "serde_json::Value",
            create(accessor = "values_json()"),
            update(accessor = "values_json()")
        ),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala"
)]
#[cfg_attr(not(feature = "event-context"), es_repo(event_context = false))]
pub(super) struct AccountRepo {
    pool: PgPool,
}

impl AccountRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[cfg(feature = "import")]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, normal_balance_type, eventually_consistent, created_at, latest_values)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
            account.values().config.eventually_consistent,
            recorded_at,
            serde_json::to_value(account.values()).expect("Failed to serialize account values"),
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, account.events_mut()).await?;
        Ok(())
    }
}
