use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, DebitOrCredit, Status},
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
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
