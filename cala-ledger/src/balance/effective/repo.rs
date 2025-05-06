use chrono::NaiveDate;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use tracing::instrument;

use crate::balance::error::BalanceError;
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, JournalId},
};

use super::data::*;

#[derive(Debug, Clone)]
pub(super) struct EffectiveBalanceRepo {
    _pool: PgPool,
}

impl EffectiveBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.find_for_update",
        skip(self, db)
    )]
    pub(super) async fn find_for_update(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        (account_ids, currencies): (Vec<AccountId>, Vec<&str>),
        effective: NaiveDate,
    ) -> Result<HashMap<(AccountId, Currency), EffectiveBalanceData>, BalanceError> {
        let rows = sqlx::query!(
            r#"
          WITH pairs AS (
            SELECT account_id, currency
            FROM (
              SELECT * FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            ) AS v
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE eventually_consistent = FALSE
          ),
          delete_balances AS (
            DELETE FROM cala_cumulative_effective_balances
            WHERE journal_id = $1
              AND (account_id, currency) IN (SELECT account_id, currency FROM pairs)
              AND effective >= $4
            RETURNING account_id, currency, effective, values
          ),
          values AS (
            SELECT 
              p.account_id,
              p.currency,
              b.values
            FROM pairs p
            LEFT JOIN LATERAL (
              SELECT DISTINCT ON (account_id, currency)
                account_id,
                currency,
                values
              FROM cala_cumulative_effective_balances
              WHERE journal_id = $1
                AND effective < $4
                AND account_id = p.account_id
                AND currency = p.currency
              ORDER BY account_id, currency, effective DESC, version DESC
            ) b ON TRUE
          )
          SELECT
            v.account_id AS "account_id!: AccountId",
            v.currency AS "currency!",
            v.values AS "values?: serde_json::Value",
            COALESCE(
              jsonb_agg(
                jsonb_build_object('effective', d.effective, 'values', d.values)
              ) FILTER (WHERE d.values IS NOT NULL),
              '[]'::jsonb
            ) AS "deleted_values!: serde_json::Value"
          FROM values v
          LEFT JOIN delete_balances d
            ON v.account_id = d.account_id AND v.currency = d.currency
          GROUP BY v.account_id, v.currency, v.values
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str],
            effective
        )
        .fetch_all(&mut **db)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let last_snapshot = row.values.map(|v| {
                serde_json::from_value::<BalanceSnapshot>(v)
                    .expect("Failed to deserialize balance snapshot")
            });

            let updates = serde_json::from_value::<Vec<SnapshotOrEntry>>(row.deleted_values)
                .expect("Failed to deserialize deleted values array");

            ret.insert(
                (
                    row.account_id,
                    row.currency.parse().expect("Could not parse currency"),
                ),
                EffectiveBalanceData::new(row.account_id, last_snapshot, updates),
            );
        }
        Ok(ret)
    }
}
