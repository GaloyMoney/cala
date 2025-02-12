use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, Postgres, QueryBuilder, Transaction};
use tracing::instrument;

use super::{account_balance::AccountBalance, error::BalanceError};
#[cfg(feature = "import")]
use cala_types::primitives::EntryId;
use cala_types::primitives::{BalanceId, DebitOrCredit};
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, JournalId},
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(super) struct BalanceRepo {
    pool: PgPool,
}

impl BalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.find_in_executor(&self.pool, journal_id, account_id, currency)
            .await
    }

    pub async fn find_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.find_in_executor(&mut **tx, journal_id, account_id, currency)
            .await
    }

    pub async fn find_in_executor(
        &self,
        executor: impl Executor<'_, Database = Postgres>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        let row = sqlx::query!(
            r#"
            SELECT h.values, a.normal_balance_type AS "normal_balance_type!: DebitOrCredit"
            FROM cala_balance_history h
            JOIN cala_current_balances c
            ON h.journal_id = c.journal_id
            AND h.account_id = c.account_id
            AND h.currency = c.currency
            AND h.version = c.latest_version
            JOIN cala_accounts a
            ON c.account_id = a.id
            WHERE c.journal_id = $1
            AND c.account_id = $2
            AND c.currency = $3
            "#,
            journal_id as JournalId,
            account_id as AccountId,
            currency.code(),
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
        from: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> Result<(Option<AccountBalance>, Option<AccountBalance>), BalanceError> {
        let rows = sqlx::query!(
            r#"
        WITH first AS (
            SELECT
              true AS first, false AS last, h.values,
              a.normal_balance_type AS "normal_balance_type!: DebitOrCredit", h.recorded_at
            FROM cala_balance_history h
            JOIN cala_accounts a
            ON h.account_id = a.id
            WHERE h.journal_id = $1
            AND h.account_id = $2
            AND h.currency = $3
            AND h.recorded_at < $4
            ORDER BY h.recorded_at DESC, h.version DESC
            LIMIT 1
        ),
        last AS (
            SELECT
              false AS first, true AS last, h.values,
              a.normal_balance_type AS "normal_balance_type!: DebitOrCredit", h.recorded_at
            FROM cala_balance_history h
            JOIN cala_accounts a
            ON h.account_id = a.id
            WHERE h.journal_id = $1
            AND h.account_id = $2
            AND h.currency = $3
            AND h.recorded_at <= COALESCE($5, NOW())
            ORDER BY h.recorded_at DESC, h.version DESC
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
        for row in rows {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values.expect("values is not null"))
                    .expect("Failed to deserialize balance snapshot");
            let balance = Some(AccountBalance::new(row.normal_balance_type, details));
            if row.first.expect("first is not null") {
                first = balance;
            } else {
                last = balance;
            }
        }
        Ok((first, last))
    }

    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        let mut journal_ids = Vec::with_capacity(ids.len());
        let mut account_ids = Vec::with_capacity(ids.len());
        let mut currencies = Vec::with_capacity(ids.len());
        for (journal_id, account_id, currency) in ids {
            journal_ids.push(uuid::Uuid::from(journal_id));
            account_ids.push(uuid::Uuid::from(account_id));
            currencies.push(currency.code().to_string());
        }

        let rows = sqlx::query!(
            r#"
            WITH balance_ids AS (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[]) 
                AS v(journal_id, account_id, currency)
            )
            SELECT 
                h.values,
                a.normal_balance_type as "normal_balance_type!: DebitOrCredit"
            FROM cala_balance_history h
            JOIN cala_current_balances c
                ON h.journal_id = c.journal_id
                AND h.account_id = c.account_id
                AND h.currency = c.currency
                AND h.version = c.latest_version
            JOIN cala_accounts a
                ON c.account_id = a.id
            JOIN balance_ids b 
                ON c.journal_id = b.journal_id
                AND c.account_id = b.account_id
                AND c.currency = b.currency"#,
            &journal_ids[..],
            &account_ids[..],
            &currencies[..],
        )
        .fetch_all(&self.pool)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            ret.insert(
                (details.journal_id, details.account_id, details.currency),
                AccountBalance::new(row.normal_balance_type, details),
            );
        }
        Ok(ret)
    }

    pub(super) async fn find_range_all(
        &self,
        ids: &[BalanceId],
        from: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> Result<HashMap<BalanceId, (Option<AccountBalance>, Option<AccountBalance>)>, BalanceError>
    {
        let mut journal_ids = Vec::with_capacity(ids.len());
        let mut account_ids = Vec::with_capacity(ids.len());
        let mut currencies = Vec::with_capacity(ids.len());
        for (journal_id, account_id, currency) in ids {
            journal_ids.push(uuid::Uuid::from(journal_id));
            account_ids.push(uuid::Uuid::from(account_id));
            currencies.push(currency.code().to_string());
        }

        let rows = sqlx::query!(
            r#"
            WITH balance_ids AS (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[]) 
                AS v(journal_id, account_id, currency)
            ),
            first AS (
                SELECT *
                FROM (
                    SELECT
                        true AS first, false AS last, h.values,
                        a.normal_balance_type AS normal_balance_type, h.recorded_at,
                        h.journal_id, h.account_id, h.currency,
                        ROW_NUMBER() OVER (
                            PARTITION BY h.journal_id, h.account_id, h.currency
                            ORDER BY h.recorded_at DESC, h.version DESC
                        ) as rn
                    FROM cala_balance_history h
                    JOIN cala_accounts a ON h.account_id = a.id
                    JOIN balance_ids b ON 
                        h.journal_id = b.journal_id 
                        AND h.account_id = b.account_id 
                        AND h.currency = b.currency
                    WHERE h.recorded_at < $4
                ) ranked
                WHERE rn = 1
            ),
            last AS (
                SELECT *
                FROM (
                    SELECT
                        false AS first, true AS last, h.values,
                        a.normal_balance_type AS normal_balance_type, h.recorded_at,
                        h.journal_id, h.account_id, h.currency,
                        ROW_NUMBER() OVER (
                            PARTITION BY h.journal_id, h.account_id, h.currency
                            ORDER BY h.recorded_at DESC, h.version DESC
                        ) as rn
                    FROM cala_balance_history h
                    JOIN cala_accounts a ON h.account_id = a.id
                    JOIN balance_ids b ON 
                        h.journal_id = b.journal_id 
                        AND h.account_id = b.account_id 
                        AND h.currency = b.currency
                    WHERE h.recorded_at <= COALESCE($5, NOW())
                ) ranked
                WHERE rn = 1
            )
            SELECT
                first, last, values, 
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                recorded_at,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM first
            UNION ALL
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                recorded_at,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM last"#,
            &journal_ids[..],
            &account_ids[..],
            &currencies[..],
            from,
            until,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let values: serde_json::Value = row.values.expect("values is not null");
            let details: BalanceSnapshot =
                serde_json::from_value(values).expect("Failed to deserialize balance snapshot");
            let balance_id = (details.journal_id, details.account_id, details.currency);
            let balance = AccountBalance::new(row.normal_balance_type, details);
            let entry = ret.entry(balance_id).or_insert((None, None));
            if row.first.expect("first is not null") {
                entry.0 = Some(balance);
            } else {
                entry.1 = Some(balance);
            }
        }
        Ok(ret)
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
        ids: HashSet<(AccountId, Currency)>,
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
        let (account_ids, currencies): (Vec<_>, Vec<_>) =
            ids.into_iter().map(|(a, c)| (a, c.code())).unzip();
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock(hashtext(concat($1::text, account_id::text, currency)))
            FROM (
            SELECT * FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            ) AS v
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE eventually_consistent = FALSE
            "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str],
        )
        .execute(&mut **db)
        .await?;
        let rows = sqlx::query!(
            r#"
            WITH pairs AS (
              SELECT account_id, currency, eventually_consistent
            FROM (
            SELECT * FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            ) AS v
            JOIN cala_accounts a
            ON account_id = a.id
            ),
          current_balances AS (
            SELECT b.journal_id, b.account_id, b.currency, b.latest_version
              FROM cala_current_balances b
              JOIN pairs p ON p.account_id = b.account_id AND p.currency = b.currency AND p.eventually_consistent = FALSE
              WHERE b.journal_id = $1
          ),
          values AS (
            SELECT p.account_id, p.currency, h.values
            FROM pairs p
            LEFT JOIN current_balances b
            ON p.account_id = b.account_id
              AND p.currency = b.currency
            LEFT JOIN cala_balance_history h
            ON b.journal_id = h.journal_id
              AND b.account_id = h.account_id
              AND b.currency = h.currency
              AND b.latest_version = h.version
            WHERE p.eventually_consistent = FALSE
          )
          SELECT account_id AS "account_id!: AccountId", currency AS "currency!", values FROM values
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str]
        ).fetch_all(&mut **db).await?;

        let mut ret = HashMap::new();
        for row in rows {
            let snapshot = row.values.map(|v| {
                serde_json::from_value::<BalanceSnapshot>(v)
                    .expect("Failed to deserialize balance snapshot")
            });
            ret.insert(
                (
                    row.account_id,
                    row.currency.parse().expect("Could not parse currency"),
                ),
                snapshot,
            );
        }
        Ok(ret)
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.insert_new_snapshots",
        skip(self, db)
    )]
    pub(crate) async fn insert_new_snapshots(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        new_balances: &[BalanceSnapshot],
    ) -> Result<(), BalanceError> {
        let mut query_builder = QueryBuilder::new(
            r#"
          WITH new_snapshots AS (
            INSERT INTO cala_balance_history (journal_id, account_id, currency, version, latest_entry_id, values)
            "#,
        );
        query_builder.push_values(new_balances, |mut builder, b| {
            builder.push_bind(b.journal_id);
            builder.push_bind(b.account_id);
            builder.push_bind(b.currency.code());
            builder.push_bind(b.version as i32);
            builder.push_bind(b.entry_id);
            builder
                .push_bind(serde_json::to_value(b).expect("Failed to serialize balance snapshot"));
        });
        query_builder.push(
            r#"
            RETURNING *
            ),
            ranked_balances AS (
              SELECT *,
                ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY version) AS rn,
                MAX(version) OVER (PARTITION BY account_id, currency) AS max
              FROM new_snapshots
            ),
            initial_balances AS (
              INSERT INTO cala_current_balances (journal_id, account_id, currency, latest_version)
              SELECT journal_id, account_id, currency, version
              FROM ranked_balances
              WHERE version = rn AND rn = max
            )
            UPDATE cala_current_balances c
            SET latest_version = n.version
            FROM ranked_balances n
            WHERE n.account_id = c.account_id
              AND n.currency = c.currency
              AND c.journal_id = n.journal_id
              AND version = max AND version != rn
              "#,
        );
        query_builder.build().execute(&mut **db).await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_balance(
        &self,
        db: &mut Transaction<'_, Postgres>,
        balance: &BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        sqlx::query!(
            r#"INSERT INTO cala_current_balances
            (journal_id, account_id, currency, latest_version, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            balance.journal_id as JournalId,
            balance.account_id as AccountId,
            balance.currency.code(),
            balance.version as i32,
            balance.created_at
        )
        .execute(&mut **db)
        .await?;
        sqlx::query!(
            r#"INSERT INTO cala_balance_history
            (journal_id, account_id, currency, version, latest_entry_id, values, recorded_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            balance.journal_id as JournalId,
            balance.account_id as AccountId,
            balance.currency.code(),
            balance.version as i32,
            balance.entry_id as EntryId,
            serde_json::to_value(&balance).expect("Failed to serialize balance snapshot"),
            balance.created_at
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub(crate) async fn load_all_for_update(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, BalanceError> {
        let rows = sqlx::query!(
            r#"
            WITH locked_accounts AS (
              SELECT 1
              FROM cala_accounts a
              WHERE a.id = $1
              FOR UPDATE
            ), locked_balances AS (
              SELECT journal_id, account_id, currency, latest_version
              FROM cala_current_balances
              WHERE journal_id = $2
              AND account_id = $1
              FOR UPDATE
            )
            SELECT h.values
            FROM cala_balance_history h
            JOIN locked_balances b
            ON b.journal_id = h.journal_id
              AND b.account_id = h.account_id
              AND b.currency = h.currency
              AND b.latest_version = h.version
        "#,
            account_id as AccountId,
            journal_id as JournalId
        )
        .fetch_all(&mut **db)
        .await?;
        let ret = rows
            .into_iter()
            .map(|row| {
                let snapshot: BalanceSnapshot = serde_json::from_value(row.values)
                    .expect("Failed to deserialize balance snapshot");
                (snapshot.currency, snapshot)
            })
            .collect();
        Ok(ret)
    }

    #[cfg(feature = "import")]
    pub async fn import_balance_update(
        &self,
        db: &mut Transaction<'_, Postgres>,
        balance: &BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        sqlx::query!(
            r#"
            UPDATE cala_current_balances
            SET latest_version = $1
            WHERE journal_id = $2 AND account_id = $3 AND currency = $4 AND latest_version = $1 - 1"#,
            balance.version as i32,
            balance.journal_id as JournalId,
            balance.account_id as AccountId,
            balance.currency.code(),
        )
        .execute(&mut **db)
        .await?;
        sqlx::query!(
            r#"INSERT INTO cala_balance_history
            (journal_id, account_id, currency, version, values, recorded_at)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            balance.journal_id as JournalId,
            balance.account_id as AccountId,
            balance.currency.code(),
            balance.version as i32,
            serde_json::to_value(&balance).expect("Failed to serialize balance snapshot"),
            balance.modified_at,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }
}
