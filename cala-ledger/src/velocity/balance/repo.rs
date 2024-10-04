use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};

use std::collections::HashMap;

use cala_types::{balance::BalanceSnapshot, velocity::Window};

use crate::primitives::*;

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
        db: &mut Transaction<'_, Postgres>,
        keys: impl Iterator<Item = &VelocityBalanceKey>,
    ) -> Result<HashMap<VelocityBalanceKey, Option<BalanceSnapshot>>, sqlx::Error> {
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
        let rows = query.fetch_all(&mut **db).await?;

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
}
