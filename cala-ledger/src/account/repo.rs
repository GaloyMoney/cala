use crate::error::CalaFault;
use es_entity::{errlanes::ResultExt, *};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, DebitOrCredit, Status},
};

use super::{entity::*, AccountKind};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Account",
    id(source = caller),
    columns(
        // No `find_by` / `list_by`: nothing reads accounts by name, and the
        // index that would back either was dropped from the migration.
        name(
            ty = "String",
            update(accessor = "values().name"),
            find_by = false
        ),
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
        is_account_set(ty = "bool", update(persist = false)),
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

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "account.update_velocity_context_values_in_op",
        skip_all
    )]
    pub async fn update_velocity_context_values_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        latest_values: VelocityContextAccountValues,
    ) -> Result<(), CalaFault> {
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

    /// What each of `account_ids` is; ids with no row are absent.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "account.find_kinds_in_op",
        skip_all,
        fields(accounts = account_ids.len())
    )]
    pub async fn find_kinds_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, AccountKind>, CalaFault> {
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!: AccountId", is_account_set
            FROM cala_accounts
            WHERE id = ANY($1)
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let kind = if row.is_account_set {
                    AccountKind::AccountSet
                } else {
                    AccountKind::Account
                };
                (row.id, kind)
            })
            .collect())
    }

    /// The cached CEL `account` values of each of `account_ids`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "account.find_context_values_in_op",
        skip_all,
        fields(accounts = account_ids.len())
    )]
    pub async fn find_context_values_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, VelocityContextAccountValues>, CalaFault> {
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!: AccountId", velocity_context_values
            FROM cala_accounts
            WHERE id = ANY($1)
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;
        let mut result = HashMap::with_capacity(rows.len());
        for row in rows {
            let values: VelocityContextAccountValues =
                serde_json::from_value(row.velocity_context_values)
                    .classify::<crate::error::CouldNotDecodeStored>()?;
            result.insert(row.id, values);
        }
        Ok(result)
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Account,
        new_events: es_entity::LastPersisted<'_, AccountEvent>,
    ) -> Result<(), CalaFault> {
        if entity.is_account_set() {
            return Ok(());
        }
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
