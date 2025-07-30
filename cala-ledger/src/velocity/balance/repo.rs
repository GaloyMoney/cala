use es_entity::DbOp;
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{balance::BalanceSnapshot, velocity::Window};

use crate::{primitives::*, velocity::error::VelocityError};

pub(super) type VelocityBalanceKey = (
    Window,
    Currency,
    JournalId,
    AccountId,
    VelocityControlId,
    VelocityLimitId,
);

#[derive(Clone)]
pub(super) struct VelocityBalanceRepo {
    _pool: PgPool,
}

impl VelocityBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }
    pub async fn find_for_update(
        &self,
        op: &mut DbOp<'_>,
        keys: impl Iterator<Item = &VelocityBalanceKey>,
    ) -> Result<HashMap<VelocityBalanceKey, Option<BalanceSnapshot>>, VelocityError> {
        let (windows, currencies, journal_ids, account_ids, control_ids, limit_ids) = keys.fold(
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        |(mut windows, mut currencies, mut journal_ids, mut account_ids, mut control_ids, mut limit_ids), &(ref window, ref currency, journal_id, account_id, control_id, limit_id)| {
            windows.push(window.inner().clone());
            currencies.push(currency.code());
            journal_ids.push(journal_id);
            account_ids.push(account_id);
            control_ids.push(control_id);
            limit_ids.push(limit_id);
            (windows, currencies, journal_ids, account_ids, control_ids, limit_ids)
        }
    );

        sqlx::query!(
        r#"
        SELECT pg_advisory_xact_lock(hashtext(concat(
            partition_window::text,
            currency,
            journal_id::text,
            account_id::text,
            velocity_control_id::text,
            velocity_limit_id::text
        )))
        FROM UNNEST(
            $1::jsonb[], 
            $2::text[], 
            $3::uuid[], 
            $4::uuid[], 
            $5::uuid[], 
            $6::uuid[]
        )
        AS v(partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id)
        "#,
        &windows[..],
        &currencies as &[&str],
        &journal_ids  as &[JournalId],
        &account_ids as &[AccountId],
        &control_ids  as &[VelocityControlId],
        &limit_ids  as &[VelocityLimitId],
    )
    .execute(&mut **op.tx())
    .await?;

        let rows = sqlx::query!(
        r#"
      WITH inputs AS (
        SELECT *
        FROM UNNEST(
          $1::jsonb[], 
          $2::text[], 
          $3::uuid[], 
          $4::uuid[], 
          $5::uuid[], 
          $6::uuid[]
        )
        AS v(partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id)
      )
      SELECT 
          i.partition_window as "partition_window!: serde_json::Value", 
          i.currency as "currency!", 
          i.journal_id as "journal_id!: JournalId", 
          i.account_id as "account_id!: AccountId", 
          i.velocity_control_id as "velocity_control_id!: VelocityControlId", 
          i.velocity_limit_id as "velocity_limit_id!: VelocityLimitId",
          h.values as "values?: serde_json::Value"
      FROM inputs i
      LEFT JOIN cala_velocity_current_balances b
        ON i.partition_window = b.partition_window
        AND i.currency = b.currency
        AND i.journal_id = b.journal_id
        AND i.account_id = b.account_id
        AND i.velocity_control_id = b.velocity_control_id
        AND i.velocity_limit_id = b.velocity_limit_id
      LEFT JOIN cala_velocity_balance_history h
        ON b.partition_window = h.partition_window
        AND b.currency = h.currency
        AND b.journal_id = h.journal_id
        AND b.account_id = h.account_id
        AND b.velocity_control_id = h.velocity_control_id
        AND b.velocity_limit_id = h.velocity_limit_id
        AND b.latest_version = h.version
      "#,
       &windows[..],
       &currencies as &[&str],
       &journal_ids  as &[JournalId],
       &account_ids as &[AccountId],
       &control_ids  as &[VelocityControlId],
       &limit_ids  as &[VelocityLimitId],
    )
    .fetch_all(&mut **op.tx())
    .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let snapshot = row.values.map(|v| {
                serde_json::from_value::<BalanceSnapshot>(v)
                    .expect("Failed to deserialize balance snapshot")
            });
            ret.insert(
                (
                    Window::from(row.partition_window),
                    row.currency.parse().expect("Could not parse currency"),
                    row.journal_id,
                    row.account_id,
                    row.velocity_control_id,
                    row.velocity_limit_id,
                ),
                snapshot,
            );
        }
        Ok(ret)
    }

    pub(crate) async fn insert_new_snapshots(
        &self,
        op: &mut DbOp<'_>,
        new_balances: HashMap<&VelocityBalanceKey, Vec<BalanceSnapshot>>,
    ) -> Result<(), VelocityError> {
        let mut journal_ids = Vec::new();
        let mut account_ids = Vec::new();
        let mut currencies = Vec::new();
        let mut velocity_control_ids = Vec::new();
        let mut velocity_limit_ids = Vec::new();
        let mut partition_windows = Vec::new();
        let mut latest_entry_ids = Vec::new();
        let mut versions = Vec::new();
        let mut values = Vec::new();

        for (key, snapshot) in new_balances
            .into_iter()
            .flat_map(|(key, snapshots)| snapshots.into_iter().map(move |snapshot| (key, snapshot)))
        {
            let (window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id) =
                key;

            journal_ids.push(*journal_id);
            account_ids.push(*account_id);
            currencies.push(currency.code());
            velocity_control_ids.push(*velocity_control_id);
            velocity_limit_ids.push(*velocity_limit_id);
            partition_windows.push(window.inner().clone());
            latest_entry_ids.push(snapshot.entry_id);
            versions.push(snapshot.version as i32);
            values.push(
                serde_json::to_value(snapshot).expect("Failed to serialize balance snapshot"),
            );
        }

        sqlx::query!(
        r#"
            WITH new_snapshots AS (
                INSERT INTO cala_velocity_balance_history (
                    journal_id, account_id, currency, velocity_control_id, velocity_limit_id, 
                    partition_window, latest_entry_id, version, values
                )
                SELECT * FROM UNNEST(
                    $1::uuid[],
                    $2::uuid[],
                    $3::text[],
                    $4::uuid[],
                    $5::uuid[],
                    $6::jsonb[],
                    $7::uuid[],
                    $8::integer[],
                    $9::jsonb[]
                ) AS t(
                    journal_id, account_id, currency, velocity_control_id, velocity_limit_id,
                    partition_window, latest_entry_id, version, values
                )
                RETURNING *
            ),
            ranked_balances AS (
                SELECT *,
                    ROW_NUMBER() OVER (
                        PARTITION BY partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id 
                        ORDER BY version
                    ) AS rn,
                    MAX(version) OVER (
                        PARTITION BY partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id
                    ) AS max
                FROM new_snapshots
            ),
            initial_balances AS (
                INSERT INTO cala_velocity_current_balances (
                    journal_id, account_id, currency, velocity_control_id, velocity_limit_id,
                    partition_window, latest_version
                )
                SELECT 
                    journal_id, account_id, currency, velocity_control_id, velocity_limit_id,
                    partition_window, version
                FROM ranked_balances
                WHERE version = rn AND rn = max
                ON CONFLICT (journal_id, account_id, currency, velocity_control_id, velocity_limit_id, partition_window)
                DO NOTHING
            )
            UPDATE cala_velocity_current_balances c
            SET latest_version = n.version
            FROM ranked_balances n
            WHERE c.journal_id = n.journal_id
                AND c.account_id = n.account_id
                AND c.currency = n.currency
                AND c.velocity_control_id = n.velocity_control_id
                AND c.velocity_limit_id = n.velocity_limit_id
                AND c.partition_window = n.partition_window
                AND version = max AND version != rn
            "#,
            &journal_ids as &[JournalId],
            &account_ids as &[AccountId],
            &currencies  as &[&str],
            &velocity_control_ids as &[VelocityControlId],
            &velocity_limit_ids as &[VelocityLimitId],
            &partition_windows[..],
            &latest_entry_ids as &[EntryId],
            &versions,
            &values,
            )
            .execute(&mut **op.tx())
            .await?;

        Ok(())
    }
}
