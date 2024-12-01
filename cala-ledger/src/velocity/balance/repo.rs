use es_entity::DbOp;
use sqlx::{PgPool, QueryBuilder, Row};

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
        let mut query_builder = QueryBuilder::new(
            r#"
            WITH inputs AS (
              SELECT *
              FROM (
            "#,
        );
        query_builder.push_values(
            keys,
            |mut builder, (window, currency, journal_id, account_id, control_id, limit_id)| {
                builder.push_bind(window);
                builder.push_bind(currency.code());
                builder.push_bind(journal_id);
                builder.push_bind(account_id);
                builder.push_bind(control_id);
                builder.push_bind(limit_id);
            },
        );
        query_builder.push(
            r#"
              ) AS v(partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id)
            ),
            locked_balances AS (
              SELECT data_source_id, b.partition_window, b.currency, b.journal_id, b.account_id, b.velocity_control_id, b.velocity_limit_id, b.latest_version
              FROM cala_velocity_current_balances b
              WHERE data_source_id = '00000000-0000-0000-0000-000000000000' AND ((partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id) IN (SELECT * FROM inputs))
              FOR UPDATE
            )
            SELECT i.partition_window, i.currency, i.journal_id, i.account_id, i.velocity_control_id, i.velocity_limit_id, h.values
            FROM inputs i
            LEFT JOIN locked_balances b
            ON i.partition_window = b.partition_window
              AND i.currency = b.currency
              AND i.journal_id = b.journal_id
              AND i.account_id = b.account_id
              AND i.velocity_control_id = b.velocity_control_id
              AND i.velocity_limit_id = b.velocity_limit_id
            LEFT JOIN cala_velocity_balance_history h
            ON b.data_source_id = h.data_source_id
              AND b.partition_window = h.partition_window
              AND b.currency = h.currency
              AND b.journal_id = h.journal_id
              AND b.account_id = h.account_id
              AND b.velocity_control_id = h.velocity_control_id
              AND b.velocity_limit_id = h.velocity_limit_id
              AND b.latest_version = h.version
        "#
        );
        let query = query_builder.build();
        let rows = query.fetch_all(&mut **op.tx()).await?;

        let mut ret = HashMap::new();
        for row in rows {
            let values: Option<serde_json::Value> = row.get("values");
            let snapshot = values.map(|v| {
                serde_json::from_value::<BalanceSnapshot>(v)
                    .expect("Failed to deserialize balance snapshot")
            });
            ret.insert(
                (
                    row.get("partition_window"),
                    row.get::<&str, _>("currency")
                        .parse()
                        .expect("Could not parse currency"),
                    row.get("journal_id"),
                    row.get("account_id"),
                    row.get("velocity_control_id"),
                    row.get("velocity_limit_id"),
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
        let mut query_builder = QueryBuilder::new(
            r#"
        WITH new_snapshots AS (
            INSERT INTO cala_velocity_balance_history (
                journal_id, account_id, currency, velocity_control_id, velocity_limit_id, partition_window, latest_entry_id, version, values
            )
        "#,
        );

        query_builder.push_values(
            new_balances.into_iter().flat_map(|(key, snapshots)| {
                snapshots.into_iter().map(move |snapshot| (key, snapshot))
            }),
            |mut builder, (key, b)| {
                let (
                    window,
                    currency,
                    journal_id,
                    account_id,
                    velocity_control_id,
                    velocity_limit_id,
                ) = key;
                builder.push_bind(journal_id);
                builder.push_bind(account_id);
                builder.push_bind(currency.code());
                builder.push_bind(velocity_control_id);
                builder.push_bind(velocity_limit_id);
                builder.push_bind(window.inner());
                builder.push_bind(b.entry_id);
                builder.push_bind(b.version as i32);
                builder.push_bind(
                    serde_json::to_value(b).expect("Failed to serialize balance snapshot"),
                );
            },
        );

        query_builder.push(
          r#"
          RETURNING *
          ),
          ranked_balances AS (
              SELECT *,
                  ROW_NUMBER() OVER (
                      PARTITION BY partition_window, currency, journal_id, account_id, velocity_control_id, velocity_limit_id ORDER BY version
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
              AND c.data_source_id = '00000000-0000-0000-0000-000000000000'
              AND version = max AND version != rn
          "#,
        );
        query_builder.build().execute(&mut **op.tx()).await?;
        Ok(())
    }
}
