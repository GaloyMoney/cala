use crate::error::CalaFault;
use es_entity::{errlanes::ResultExt, *};
use sqlx::PgPool;
use std::collections::HashMap;

use cala_types::velocity::VelocityContextAccountValues;

use crate::outbox::{OutboxEventPayload, OutboxPublisher};

use super::entity::*;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Calculation",
    id(source = caller),
    columns(
        journal_id(ty = "JournalId", update(persist = false)),
        code(ty = "String", update(accessor = "values().code", persist = false)),
        name(
            ty = "String",
            update(accessor = "values().name", persist = false),
            find_by = false
        ),
        backfill_complete(
            ty = "bool",
            create(accessor = "backfill_complete()"),
            update(accessor = "backfill_complete()"),
            find_by = false
        ),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct CalculationRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl CalculationRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.list_for_journal",
        skip_all
    )]
    pub async fn list_for_journal(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
    ) -> Result<Vec<Calculation>, CalaFault> {
        let rows = op
            .into_executor()
            .fetch_all(sqlx::query_as!(
                GenericEvent,
                r#"WITH calculations AS (
              SELECT id, created_at AS entity_created_at
              FROM cala_calculations
              WHERE journal_id = $1
            )
            SELECT c.id as entity_id, e.sequence, e.event, NULL as "context: es_entity::ContextData", e.recorded_at, NULL::jsonb as "forgettable_payload?"
            FROM calculations c
            JOIN cala_calculation_events e ON c.id = e.id
            ORDER BY c.id, e.sequence"#,
                journal_id as JournalId,
            ))
            .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n::<Calculation>(rows, n)?.0;
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.journal_exists",
        skip_all
    )]
    pub async fn journal_exists(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
    ) -> Result<bool, CalaFault> {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM cala_journals WHERE id = $1) AS "exists!""#,
            journal_id as JournalId,
        )
        .fetch_one(op.as_executor())
        .await?;
        Ok(exists)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.any_exist_in_op",
        skip_all
    )]
    pub async fn any_exist_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
    ) -> Result<bool, CalaFault> {
        let exists =
            sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM cala_calculations) AS "exists!""#)
                .fetch_one(op.as_executor())
                .await?;
        Ok(exists)
    }

    /// Every account set in `journal_id` reachable from each of
    /// `account_ids` through set membership, synchronous and eventually
    /// consistent alike.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.fetch_ancestors_in_op",
        skip_all,
        fields(accounts = account_ids.len())
    )]
    pub async fn fetch_ancestors_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountId>>, CalaFault> {
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE seed AS (
                SELECT m.member_account_id AS account_id, m.account_set_id
                FROM cala_account_set_member_accounts m
                WHERE m.member_account_id = ANY($2)
            ),
            ancestors AS (
                SELECT account_id, account_set_id FROM seed
                UNION
                SELECT a.account_id, e.account_set_id
                FROM ancestors a
                JOIN cala_account_set_member_account_sets e
                  ON e.member_account_set_id = a.account_set_id
            )
            SELECT
                a.account_set_id AS "account_set_id!: AccountId",
                a.account_id AS "member_account_id!: AccountId"
            FROM ancestors a
            JOIN cala_account_sets s
              ON s.id = a.account_set_id AND s.journal_id = $1
            "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut result: HashMap<AccountId, Vec<AccountId>> = HashMap::new();
        for row in rows {
            result
                .entry(row.member_account_id)
                .or_default()
                .push(row.account_set_id);
        }
        Ok(result)
    }

    /// The cached CEL `account` values of each of `account_ids`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.account_context_values_in_op",
        skip_all,
        fields(accounts = account_ids.len())
    )]
    pub async fn account_context_values_in_op(
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
        entity: &Calculation,
        new_events: es_entity::LastPersisted<'_, CalculationEvent>,
    ) -> Result<(), CalaFault> {
        let calculation_id = entity.id;
        let payloads = new_events.map(|persisted| match &persisted.event {
            CalculationEvent::Initialized { values } => OutboxEventPayload::CalculationCreated {
                calculation: values.clone(),
            },
            CalculationEvent::BackfillCompleted {
                activation_sequence,
            } => OutboxEventPayload::CalculationBackfillCompleted {
                calculation_id,
                account_ids: None,
                activation_sequence: *activation_sequence,
            },
        });
        self.publisher.publish_all(op, payloads).await?;
        Ok(())
    }
}
