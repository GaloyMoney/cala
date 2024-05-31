use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};
use tracing::instrument;

use super::{account_balance::AccountBalance, error::BalanceError};
use cala_types::primitives::{BalanceId, DebitOrCredit};
#[cfg(feature = "import")]
use cala_types::primitives::{DataSourceId, EntryId};
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
        let row = sqlx::query!(
            r#"
            SELECT h.values, a.normal_balance_type AS "normal_balance_type!: DebitOrCredit"
            FROM cala_balance_history h
            JOIN cala_current_balances c
            ON h.data_source_id = c.data_source_id
            AND h.journal_id = c.journal_id
            AND h.account_id = c.account_id
            AND h.currency = c.currency
            AND h.version = c.latest_version
            JOIN cala_accounts a
            ON c.data_source_id = a.data_source_id
            AND c.account_id = a.id
            WHERE c.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND c.journal_id = $1
            AND c.account_id = $2
            AND c.currency = $3
            "#,
            journal_id as JournalId,
            account_id as AccountId,
            currency.code(),
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            Ok(AccountBalance {
                balance_type: row.normal_balance_type,
                details,
            })
        } else {
            Err(BalanceError::NotFound(journal_id, account_id, currency))
        }
    }

    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        let mut query_builder = QueryBuilder::new(
            r#"
            SELECT h.values, a.normal_balance_type
            FROM cala_balance_history h
            JOIN cala_current_balances c
            ON h.data_source_id = c.data_source_id
            AND h.journal_id = c.journal_id
            AND h.account_id = c.account_id
            AND h.currency = c.currency
            AND h.version = c.latest_version
            JOIN cala_accounts a
            ON c.data_source_id = a.data_source_id
            AND c.account_id = a.id
            WHERE c.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND (c.journal_id, c.account_id, c.currency) IN"#,
        );
        query_builder.push_tuples(ids, |mut builder, (journal_id, account_id, currency)| {
            builder.push_bind(journal_id);
            builder.push_bind(account_id);
            builder.push_bind(currency.code());
        });
        let query = query_builder.build();
        let rows = query.fetch_all(&self.pool).await?;
        let mut ret = HashMap::new();
        for row in rows {
            let values: serde_json::Value = row.get("values");
            let details: BalanceSnapshot =
                serde_json::from_value(values).expect("Failed to deserialize balance snapshot");
            let normal_balance_type: DebitOrCredit = row.get("normal_balance_type");
            ret.insert(
                (details.journal_id, details.account_id, details.currency),
                AccountBalance {
                    details,
                    balance_type: normal_balance_type,
                },
            );
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
        let mut query_builder = QueryBuilder::new(
            r#"
        WITH pairs AS (
          SELECT account_id, currency, eventually_consistent FROM ("#,
        );
        query_builder.push_values(ids, |mut builder, (id, currency)| {
            builder.push_bind(id);
            builder.push_bind(currency.code());
        });
        query_builder.push(
            r#"
            ) AS v(account_id, currency)
            JOIN cala_accounts a
            ON a.data_source_id = '00000000-0000-0000-0000-000000000000' AND account_id = a.id
          ),
          locked_balances AS (
            SELECT b.data_source_id, b.journal_id, b.account_id, b.currency, b.latest_version
              FROM cala_current_balances b
              JOIN pairs p ON p.account_id = b.account_id AND p.currency = b.currency AND p.eventually_consistent = FALSE
              WHERE b.data_source_id = '00000000-0000-0000-0000-000000000000'
              AND b.journal_id = "#,
        );
        query_builder.push_bind(journal_id);
        query_builder.push(
            r#"
            FOR UPDATE OF b
          ),
          values AS (
            SELECT b.data_source_id, p.account_id, p.currency, h.values
            FROM pairs p
            LEFT JOIN locked_balances b
            ON p.account_id = b.account_id
              AND p.currency = b.currency
            LEFT JOIN cala_balance_history h
            ON b.data_source_id = h.data_source_id
              AND b.journal_id = h.journal_id
              AND b.account_id = h.account_id
              AND b.currency = h.currency
              AND b.latest_version = h.version
            WHERE p.eventually_consistent = FALSE
          ),
          locked_accounts AS (
            SELECT 1
            FROM values v
            JOIN cala_accounts a
            ON v.data_source_id = a.data_source_id AND v.account_id = a.id
            WHERE v.values IS NULL
            FOR UPDATE
          )
          SELECT account_id, currency, values FROM values
        "#,
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
                    row.get("account_id"),
                    row.get::<&str, _>("currency")
                        .parse()
                        .expect("Could not parse currency"),
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
            initial_balances AS (
              INSERT INTO cala_current_balances (journal_id, account_id, currency, latest_version)
              SELECT journal_id, account_id, currency, version
              FROM new_snapshots
              WHERE version = 1
            ),
            ranked_balances AS (
              SELECT *, ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY version DESC) AS rn
              FROM new_snapshots
              WHERE version != 1
            )
            UPDATE cala_current_balances c
            SET latest_version = n.version
            FROM ranked_balances n
            WHERE n.account_id = c.account_id
              AND n.currency = c.currency
              AND c.data_source_id = '00000000-0000-0000-0000-000000000000'
              AND c.journal_id = n.journal_id
              AND rn = 1"#,
        );
        query_builder.build().execute(&mut **db).await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_balance(
        &self,
        db: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        balance: &BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        sqlx::query!(
            r#"INSERT INTO cala_current_balances
            (data_source_id, journal_id, account_id, currency, latest_version, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
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
            (data_source_id, journal_id, account_id, currency, version, latest_entry_id, values, recorded_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            origin as DataSourceId,
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

    #[cfg(feature = "import")]
    pub async fn import_balance_update(
        &self,
        db: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        balance: &BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        sqlx::query!(
            r#"
            UPDATE cala_current_balances
            SET latest_version = $1
            WHERE data_source_id = $2 AND journal_id = $3 AND account_id = $4 AND currency = $5 AND latest_version = $1 - 1"#,
            balance.version as i32,
            origin as DataSourceId,
            balance.journal_id as JournalId,
            balance.account_id as AccountId,
            balance.currency.code(),
        )
        .execute(&mut **db)
        .await?;
        sqlx::query!(
            r#"INSERT INTO cala_balance_history
            (data_source_id, journal_id, account_id, currency, version, values, recorded_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            origin as DataSourceId,
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
