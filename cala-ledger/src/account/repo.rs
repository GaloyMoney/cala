use es_entity::*;
use sqlx::PgPool;

use crate::primitives::{AccountId, DataSourceId, DebitOrCredit};

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
        velocity_context_values(
            ty = "VelocityContextAccountValues",
            create(accessor = "context_values()"),
            update(accessor = "context_values()")
        ),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala"
)]
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
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, normal_balance_type, eventually_consistent, created_at, velocity_context_values)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
            account.values().config.eventually_consistent,
            recorded_at,
            account.context_values() as VelocityContextAccountValues,
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, account.events_mut()).await?;
        Ok(())
    }

    pub async fn update_velocity_context_values_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        latest_values: impl Into<VelocityContextAccountValues>,
    ) -> Result<(), AccountError> {
        let latest_values: VelocityContextAccountValues = latest_values.into();
        let account_id = latest_values.id;

        sqlx::query!(
            r#"UPDATE cala_accounts
            SET velocity_context_values = $2
            WHERE id = $1"#,
            account_id as AccountId,
            latest_values as VelocityContextAccountValues
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    pub async fn update_all_velocity_context_values_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        latest_values_list: Vec<impl Into<VelocityContextAccountValues>>,
    ) -> Result<(), AccountError> {
        if latest_values_list.is_empty() {
            return Ok(());
        }

        let mut id_collection: Vec<AccountId> = Vec::new();
        let mut latest_values_collection: Vec<VelocityContextAccountValues> = Vec::new();

        for latest_values in latest_values_list {
            let latest_values: VelocityContextAccountValues = latest_values.into();
            id_collection.push(latest_values.id);
            latest_values_collection.push(latest_values);
        }

        sqlx::query!(
            r#"UPDATE cala_accounts
            SET velocity_context_values = unnested.latest_values
            FROM UNNEST($1::UUID[], $2::JSONB[]) AS unnested(id, latest_values)
            WHERE cala_accounts.id = unnested.id"#,
            &id_collection as &[AccountId],
            &latest_values_collection as &[VelocityContextAccountValues]
        )
        .execute(op.as_executor())
        .await?;

        Ok(())
    }
}
