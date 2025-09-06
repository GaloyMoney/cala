use sqlx::PgPool;
use tracing::instrument;

use super::{account_balance::AccountBalance, error::BalanceError};
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{AccountId, BalanceId, Currency, DebitOrCredit, EntryId, JournalId},
};
use std::collections::HashMap;

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
        self.find_in_op(&self.pool, journal_id, account_id, currency)
            .await
    }

    pub async fn find_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        let row = op
            .into_executor()
            .fetch_optional(sqlx::query!(
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
            ))
            .await?;

        if let Some(row) = row {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            Ok(AccountBalance::new(row.normal_balance_type, details))
        } else {
            Err(BalanceError::NotFound(journal_id, account_id, currency))
        }
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

    #[instrument(
        level = "trace",
        name = "cala_ledger.balances.find_for_update",
        skip(self, op)
    )]
    pub(super) async fn find_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<(AccountId, Currency), Option<BalanceSnapshot>>, BalanceError> {
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
        .execute(op.as_executor())
        .await?;
        let rows = sqlx::query!(
            r#"
            SELECT
                v.account_id AS "account_id!: AccountId",
                v.currency AS "currency!",
                b.latest_values
            FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            JOIN cala_accounts a ON a.id = v.account_id AND a.eventually_consistent = FALSE
            LEFT JOIN cala_current_balances b
                ON b.journal_id = $1
                AND b.account_id = v.account_id
                AND b.currency = v.currency
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str]
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let snapshot = row.latest_values.map(|v| {
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
        skip(self, op, new_balances)
        fields(n_new_balances)
    )]
    pub(crate) async fn insert_new_snapshots(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        new_balances: &[BalanceSnapshot],
    ) -> Result<(), BalanceError> {
        tracing::Span::current().record(
            "n_new_balances",
            tracing::field::display(new_balances.len()),
        );

        let mut journal_ids = Vec::with_capacity(new_balances.len());
        let mut account_ids = Vec::with_capacity(new_balances.len());
        let mut entry_ids = Vec::with_capacity(new_balances.len());
        let mut currencies = Vec::with_capacity(new_balances.len());
        let mut versions = Vec::with_capacity(new_balances.len());
        let mut values = Vec::with_capacity(new_balances.len());

        for balance in new_balances {
            journal_ids.push(balance.journal_id);
            account_ids.push(balance.account_id);
            entry_ids.push(balance.entry_id);
            currencies.push(balance.currency.code());
            versions.push(balance.version as i32);
            values
                .push(serde_json::to_value(balance).expect("Failed to serialize balance snapshot"));
        }

        sqlx::query!(
            r#"
        WITH new_snapshots AS (
            INSERT INTO cala_balance_history (
                journal_id, account_id, currency, version, latest_entry_id, values
            )
            SELECT * FROM UNNEST (
                $1::uuid[],
                $2::uuid[],
                $3::text[],
                $4::int4[],
                $5::uuid[],
                $6::jsonb[]
            )
            RETURNING *
        ),
        ranked_balances AS (
            SELECT *,
                   ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY version) AS rn,
                   MAX(version) OVER (PARTITION BY account_id, currency) AS max
            FROM new_snapshots
        ),
        initial_balances AS (
            INSERT INTO cala_current_balances (journal_id, account_id, currency, latest_version, latest_values)
            SELECT journal_id, account_id, currency, version, values
            FROM ranked_balances
            WHERE version = rn AND rn = max
        )
        UPDATE cala_current_balances c
        SET latest_version = n.version, latest_values = n.values
        FROM ranked_balances n
        WHERE n.account_id = c.account_id
          AND n.currency = c.currency
          AND c.journal_id = n.journal_id
          AND version = max AND version != rn
        "#,
            &journal_ids as &[JournalId],
            &account_ids as &[AccountId],
            &currencies as &[&str],
            &versions as &[i32],
            &entry_ids as &[EntryId],
            &values
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn import_balance(
        &self,
        op: &mut impl es_entity::AtomicOperation,
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
        .execute(op.as_executor())
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
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    pub(crate) async fn load_all_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
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
        .fetch_all(op.as_executor())
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
        op: &mut impl es_entity::AtomicOperation,
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
        .execute(op.as_executor())
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
        .execute(op.as_executor())
        .await?;
        Ok(())
    }
}
