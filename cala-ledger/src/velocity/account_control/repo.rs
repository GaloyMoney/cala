use sqlx::PgPool;
use tracing::instrument;

use crate::primitives::{AccountId, VelocityControlId};

use super::{super::error::*, value::*};

#[derive(Debug, Clone)]
pub struct AccountControlRepo {
    _pool: PgPool,
}

impl AccountControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "debug",
        name = "account_control.create_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub async fn create_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        control: AccountVelocityControl,
    ) -> Result<(), VelocityError> {
        sqlx::query!(
            r#"INSERT INTO cala_velocity_account_controls (account_id, velocity_control_id, values)
            VALUES ($1, $2, $3)"#,
            control.account_id as AccountId,
            control.control_id as VelocityControlId,
            serde_json::to_value(control).expect("Failed to serialize control values"),
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    /// Batched counterpart of [`Self::create_in_op`]: one multi-row `INSERT`
    /// for every `(account_id, velocity_control_id, values)` triple instead
    /// of one `INSERT` per row.
    ///
    /// `controls` is sorted by `account_id` so that concurrent callers
    /// insert overlapping rows in the same order: two multi-row `INSERT`s
    /// touching the same `(account_id, velocity_control_id)` keys in
    /// opposite order can otherwise deadlock against each other on that
    /// unique index mid-statement.
    ///
    /// The sort is done in Rust rather than with a SQL `ORDER BY` because
    /// the `UNNEST` is join-free: nothing can reorder a bare `UNNEST`
    /// scan, so rows are inserted in the array order they are supplied in.
    #[instrument(
        level = "debug",
        name = "account_control.create_all_in_op",
        skip_all,
        fields(count = controls.len()),
        err(level = "warn")
    )]
    pub async fn create_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        mut controls: Vec<AccountVelocityControl>,
    ) -> Result<(), VelocityError> {
        if controls.is_empty() {
            return Ok(());
        }

        controls.sort_unstable_by_key(|c| c.account_id);

        let mut account_ids = Vec::with_capacity(controls.len());
        let mut control_ids = Vec::with_capacity(controls.len());
        let mut values = Vec::with_capacity(controls.len());
        for control in controls {
            account_ids.push(control.account_id);
            control_ids.push(control.control_id);
            values.push(serde_json::to_value(control).expect("Failed to serialize control values"));
        }

        sqlx::query!(
            r#"
            INSERT INTO cala_velocity_account_controls (account_id, velocity_control_id, values)
            SELECT account_id, velocity_control_id, values
            FROM UNNEST($1::uuid[], $2::uuid[], $3::jsonb[])
                AS v(account_id, velocity_control_id, values)
            "#,
            &account_ids as &[AccountId],
            &control_ids as &[VelocityControlId],
            &values,
        )
        .execute(op.as_executor())
        .await?;

        Ok(())
    }
}
