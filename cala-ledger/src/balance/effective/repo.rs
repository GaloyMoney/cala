use chrono::NaiveDate;
use sqlx::{Executor, PgPool, Postgres, QueryBuilder, Transaction};
use std::collections::HashMap;
use tracing::instrument;

use crate::balance::{account_balance::AccountBalance, error::BalanceError};
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, DebitOrCredit, JournalId},
};

use super::data::*;

#[derive(Debug, Clone)]
pub(super) struct EffectiveBalanceRepo {
    pool: PgPool,
}

impl EffectiveBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        self.find_in_executor(&self.pool, journal_id, account_id, currency, date)
            .await
    }

    async fn find_in_executor(
        &self,
        executor: impl Executor<'_, Database = Postgres>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        let row = sqlx::query!(
            r#"
            SELECT values, a.normal_balance_type AS "normal_balance_type!: DebitOrCredit"
            FROM cala_cumulative_effective_balances
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE journal_id = $1
            AND account_id = $2
            AND currency = $3
            AND effective <= $4
            ORDER BY effective DESC, version DESC
            LIMIT 1
            "#,
            journal_id as JournalId,
            account_id as AccountId,
            currency.code(),
            date
        )
        .fetch_optional(executor)
        .await?;

        if let Some(row) = row {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            Ok(AccountBalance::new(row.normal_balance_type, details))
        } else {
            Err(BalanceError::NotFound(journal_id, account_id, currency))
        }
    }

    pub(super) async fn find_range(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<(Option<AccountBalance>, Option<AccountBalance>, u32), BalanceError> {
        let rows = sqlx::query!(
            r#"
        WITH first AS (
            SELECT
              true AS first, false AS last, values,
              a.normal_balance_type AS "normal_balance_type!: DebitOrCredit",
              all_time_version
            FROM cala_cumulative_effective_balances
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE journal_id = $1
            AND account_id = $2
            AND currency = $3
            AND effective < $4
            ORDER BY effective DESC, version DESC
            LIMIT 1
        ),
        last AS (
            SELECT
              false AS first, true AS last, values,
              a.normal_balance_type AS "normal_balance_type!: DebitOrCredit",
              all_time_version
            FROM cala_cumulative_effective_balances
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE journal_id = $1
            AND account_id = $2
            AND currency = $3
            AND effective <= COALESCE($5, NOW()::DATE)
            ORDER BY effective DESC, version DESC
            LIMIT 1
        )
        SELECT * FROM first
        UNION ALL
        SELECT * FROM last
        "#,
            journal_id as JournalId,
            account_id as AccountId,
            currency.code(),
            from,
            until,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut first = None;
        let mut last = None;
        let mut first_version = 0;
        let mut last_version = 0;
        for row in rows {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values.expect("values is not null"))
                    .expect("Failed to deserialize balance snapshot");
            let balance = Some(AccountBalance::new(row.normal_balance_type, details));
            if row.first.expect("first is not null") {
                first = balance;
                first_version = row.all_time_version.expect("all_time_version") as u32;
            } else {
                last = balance;
                last_version = row.all_time_version.expect("all_time_version") as u32;
            }
        }
        Ok((first, last, last_version - first_version))
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.effective.find_for_update",
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
              b.values,
              b.all_time_version
            FROM pairs p
            LEFT JOIN LATERAL (
              SELECT DISTINCT ON (account_id, currency)
                account_id,
                currency,
                values,
                all_time_version
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
            v.all_time_version AS "all_time_version?: i32",
            COALESCE(
              jsonb_agg(
                jsonb_build_object('effective', d.effective, 'values', d.values)
              ) FILTER (WHERE d.values IS NOT NULL),
              '[]'::jsonb
            ) AS "deleted_values!: serde_json::Value"
          FROM values v
          LEFT JOIN delete_balances d
            ON v.account_id = d.account_id AND v.currency = d.currency
          GROUP BY v.account_id, v.currency, v.values, v.all_time_version
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

            let currency = row.currency.parse().expect("Failed to parse currency");
            ret.insert(
                (row.account_id, currency),
                EffectiveBalanceData::new(
                    row.account_id,
                    currency,
                    last_snapshot,
                    row.all_time_version.map(|v| v as u32).unwrap_or(0),
                    updates,
                ),
            );
        }
        Ok(ret)
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.effective.insert_new_snapshots",
        skip(self, db, data)
    )]
    pub(crate) async fn insert_new_snapshots(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        data: HashMap<(AccountId, Currency), EffectiveBalanceData<'_>>,
    ) -> Result<(), BalanceError> {
        let mut query_builder = QueryBuilder::new(
            r#"
            INSERT INTO cala_cumulative_effective_balances (
              journal_id, account_id, currency, effective, version, all_time_version, latest_entry_id, updated_at, created_at, values
            )
            "#,
        );
        query_builder.push_values(
            data.into_values().flat_map(|d| d.into_updates()),
            |mut builder, (account_id, currency, effective, snapshot, all_time_version)| {
                builder.push_bind(journal_id);
                builder.push_bind(account_id);
                builder.push_bind(currency.code());
                builder.push_bind(effective);
                builder.push_bind(snapshot.version as i32);
                builder.push_bind(all_time_version as i32);
                builder.push_bind(snapshot.entry_id);
                builder.push_bind(snapshot.modified_at);
                builder.push_bind(snapshot.created_at);
                builder.push_bind(
                    serde_json::to_value(snapshot).expect("Failed to serialize balance snapshot"),
                );
            },
        );
        query_builder.build().execute(&mut **db).await?;
        Ok(())
    }
}
