use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::{error::CalaFault, primitives::*};

/// An open backfill unit: a Global calculation (`account_ids == None`) or
/// the attachments created together by one `attach*` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculationBackfill {
    /// Equals the id of the job that runs the backfill.
    pub id: CalculationBackfillId,
    pub calculation_id: CalculationId,
    pub account_ids: Option<Vec<AccountId>>,
    /// The last outbox sequence folded and committed.
    pub cursor: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(super) struct BackfillRepo {
    pool: PgPool,
}

impl BackfillRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_backfill.create_in_op",
        skip_all
    )]
    pub async fn create_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationBackfillId,
        calculation_id: CalculationId,
        account_ids: Option<&[AccountId]>,
    ) -> Result<(), CalaFault> {
        sqlx::query!(
            r#"
            INSERT INTO cala_calculation_backfills (id, calculation_id, account_ids)
            VALUES ($1, $2, $3)
            "#,
            id as CalculationBackfillId,
            calculation_id as CalculationId,
            account_ids as Option<&[AccountId]>,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_backfill.list_open",
        skip_all
    )]
    pub async fn list_open(
        &self,
        calculation_id: CalculationId,
    ) -> Result<Vec<CalculationBackfill>, CalaFault> {
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id: CalculationBackfillId",
                   calculation_id AS "calculation_id: CalculationId",
                   account_ids AS "account_ids: Vec<AccountId>",
                   cursor,
                   created_at
            FROM cala_calculation_backfills
            WHERE calculation_id = $1 AND completed_at IS NULL
            ORDER BY created_at, id
            "#,
            calculation_id as CalculationId,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| CalculationBackfill {
                id: row.id,
                calculation_id: row.calculation_id,
                account_ids: row.account_ids,
                cursor: row.cursor as u64,
                created_at: row.created_at,
            })
            .collect())
    }

    /// SHARED side of the handoff lock, held to the end of the operation.
    /// The tip takes it as the first statement of every flush.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_backfill.lock_handoff_shared_in_op",
        skip_all
    )]
    pub async fn lock_handoff_shared_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
    ) -> Result<(), CalaFault> {
        sqlx::query("SELECT pg_advisory_xact_lock_shared($1::int4, 0)")
            .bind(super::CALCULATION_HANDOFF_LOCK_CLASS)
            .execute(op.as_executor())
            .await?;
        Ok(())
    }
}
