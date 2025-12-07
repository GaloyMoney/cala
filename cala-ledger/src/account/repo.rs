use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, DataSourceId, DebitOrCredit, Status},
};

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
        status(ty = "Status", update(accessor = "values().status")),
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
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct AccountRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl AccountRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    #[cfg(feature = "import")]
    #[instrument(name = "account.import_in_op", skip_all, err(level = "warn"))]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, normal_balance_type, status, eventually_consistent, created_at, 
        velocity_context_values)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
            account.values().status as Status,
            account.values().config.eventually_consistent,
            recorded_at,
            account.context_values() as VelocityContextAccountValues,
        )
        .execute(op.as_executor())
        .await?;
        let n_events = self.persist_events(op, account.events_mut()).await?;
        self.publish(op, account, account.events().last_persisted(n_events))
            .await?;

        Ok(())
    }

    #[instrument(
        name = "account.update_velocity_context_values_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn update_velocity_context_values_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        latest_values: VelocityContextAccountValues,
    ) -> Result<(), AccountError> {
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

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Account,
        new_events: es_entity::LastPersisted<'_, AccountEvent>,
    ) -> Result<(), AccountError> {
        self.publisher.publish_all(op, entity, new_events).await?;
        Ok(())
    }
}
