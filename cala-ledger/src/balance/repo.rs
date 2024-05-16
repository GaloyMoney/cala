use sqlx::{PgPool, Postgres, QueryBuilder, Row, Transaction};
use tracing::instrument;

use super::error::BalanceError;
#[cfg(feature = "import")]
use cala_types::primitives::DataSourceId;
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, Currency, JournalId},
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(super) struct BalanceRepo {
    _pool: PgPool,
}

impl BalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.find_for_update",
        skip(self, tx)
    )]
    pub(super) async fn find_for_update<'a>(
        &self,
        tx: &mut Transaction<'a, Postgres>,
        journal_id: JournalId,
        ids: HashSet<(AccountId, Currency)>,
    ) -> Result<HashMap<(AccountId, Currency), BalanceSnapshot>, BalanceError> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
          SELECT h.values
          FROM cala_balance_history h
          JOIN ( SELECT data_source_id, journal_id, account_id, currency, latest_version
                 FROM cala_current_balances
                 WHERE data_source_id = '00000000-0000-0000-0000-000000000000' AND journal_id = "#,
        );
        query_builder.push_bind(journal_id);
        query_builder.push(r#" AND (account_id, currency) IN"#);
        query_builder.push_tuples(ids, |mut builder, (id, currency)| {
            builder.push_bind(id);
            builder.push_bind(currency.code());
        });
        query_builder.push(
            r#"
        FOR UPDATE ) b
        ON b.data_source_id = h.data_source_id
          AND b.journal_id = h.journal_id
          AND b.account_id = h.account_id
          AND b.currency = h.currency
          AND b.latest_version = h.version
        "#,
        );
        let query = query_builder.build();
        let rows = query.fetch_all(&mut **tx).await?;

        let mut ret = HashMap::new();
        for row in rows {
            let values = row.get::<serde_json::Value, _>("values");
            let snapshot: BalanceSnapshot =
                serde_json::from_value(values).expect("Failed to deserialize balance snapshot");
            ret.insert((snapshot.account_id, snapshot.currency), snapshot);
        }
        Ok(ret)
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.insert_new_snapshots",
        skip(self, tx)
    )]
    pub(crate) async fn insert_new_snapshots(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        journal_id: JournalId,
        new_balances: &[BalanceSnapshot],
    ) -> Result<(), BalanceError> {
        let mut to_insert = HashMap::new();
        let mut to_update = HashMap::new();
        let mut previous_versions = HashMap::new();
        for BalanceSnapshot {
            account_id,
            currency,
            version,
            ..
        } in new_balances.iter()
        {
            if *version == 1 {
                to_insert.insert((account_id, currency), version);
            } else {
                to_update.insert((account_id, currency), version);
                if previous_versions.contains_key(&(account_id, currency)) {
                    continue;
                }
                previous_versions.insert((account_id, currency), version - 1);
            }
        }
        if !to_insert.is_empty() {
            let mut query_builder = QueryBuilder::new(
                r#"INSERT INTO cala_current_balances
                  (journal_id, account_id, currency, latest_version)"#,
            );
            query_builder.push_values(
                to_insert.iter(),
                |mut builder, ((account_id, currency), version)| {
                    builder.push_bind(journal_id);
                    builder.push_bind(account_id);
                    builder.push_bind(currency.code());
                    builder.push_bind(**version as i32);
                },
            );
            query_builder.build().execute(&mut **tx).await?;
        }
        if !to_update.is_empty() {
            let expected_updates = to_update.len();
            let mut query_builder =
                QueryBuilder::new(r#"UPDATE cala_current_balances SET latest_version = CASE"#);
            let mut bind_numbers = HashMap::new();
            let mut next_bind_number = 1;
            for ((account_id, currency), version) in to_update {
                bind_numbers.insert((account_id, currency), next_bind_number);
                next_bind_number += 3;
                query_builder.push(" WHEN account_id = ");
                query_builder.push_bind(account_id);
                query_builder.push(" AND currency = ");
                query_builder.push_bind(currency.code());
                query_builder.push(" THEN ");
                query_builder.push_bind(*version as i32);
            }
            query_builder.push(" END WHERE data_source_id = '00000000-0000-0000-0000-000000000000' AND journal_id = ");
            query_builder.push_bind(journal_id);
            query_builder.push(" AND (account_id, currency, version) IN");
            query_builder.push_tuples(
                previous_versions,
                |mut builder, ((account_id, currency), version)| {
                    let n = bind_numbers.remove(&(account_id, currency)).unwrap();
                    builder.push(format!("${}, ${}", n, n + 1));
                    builder.push_bind(version as i32);
                },
            );
            let result = query_builder.build().execute(&mut **tx).await?;
            if result.rows_affected() != (expected_updates as u64) {
                return Err(BalanceError::OptimisticLockingError);
            }
        }

        let mut query_builder = QueryBuilder::new(
            r#"INSERT INTO cala_balance_history (
                 journal_id, account_id, currency, version, values)
            "#,
        );
        query_builder.push_values(new_balances, |mut builder, b| {
            builder.push_bind(b.journal_id);
            builder.push_bind(b.account_id);
            builder.push_bind(b.currency.code());
            builder.push_bind(b.version as i32);
            builder
                .push_bind(serde_json::to_value(b).expect("Failed to serialize balance snapshot"));
        });
        query_builder.build().execute(&mut **tx).await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_balance(
        &self,
        tx: &mut Transaction<'_, Postgres>,
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
        .execute(&mut **tx)
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
            balance.created_at
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_balance_update(
        &self,
        tx: &mut Transaction<'_, Postgres>,
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
        .execute(&mut **tx)
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
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
