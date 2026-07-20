use chrono::NaiveDate;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::{
    balance::{
        account_balance::{AccountBalance, BalanceRange},
        cursor::{AccountBalanceByCurrencyCursor, AccountBalanceCursor},
        error::BalanceError,
    },
    outbox::OutboxPublisher,
};
use cala_types::{
    balance::{BalanceSnapshot, EffectiveBalanceSnapshot},
    outbox::OutboxEventPayload,
    primitives::{AccountId, BalanceId, Currency, DebitOrCredit, EntryId, JournalId},
};

use super::data::*;

type BalanceRangeResult =
    HashMap<BalanceId, (Option<AccountBalance>, u32, Option<AccountBalance>, u32)>;

#[derive(Debug, Clone)]
pub(super) struct EffectiveBalanceRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl EffectiveBalanceRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        self.find_in_op(&self.pool, journal_id, account_id, currency, date)
            .await
    }

    #[instrument(name = "effective_balance.find_in_op", skip_all, err(level = "warn"))]
    pub async fn find_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        let row = op
            .into_executor()
            .fetch_optional(sqlx::query!(
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

    #[instrument(name = "effective_balance.find_range", skip_all, err(level = "warn"))]
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

    #[instrument(name = "cala_ledger.balances.effective.find_all", skip_all)]
    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
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
              SELECT journal_id, account_id, currency, normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[])
                AS v(journal_id, account_id, currency)
              ) AS v
              JOIN cala_accounts a
              ON account_id = a.id
            )
            SELECT
                values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                h.journal_id as "journal_id: JournalId",
                h.account_id as "account_id: AccountId",
                h.currency
            FROM balance_ids
            JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency)
                    journal_id, account_id, currency, values
                FROM cala_cumulative_effective_balances
                WHERE journal_id = balance_ids.journal_id
                  AND account_id = balance_ids.account_id
                  AND currency = balance_ids.currency
                  AND effective <= $4
                ORDER BY journal_id, account_id, currency, effective DESC, version DESC
            ) h ON TRUE
            "#,
            &journal_ids[..],
            &account_ids[..],
            &currencies[..],
            date,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let details: BalanceSnapshot =
                serde_json::from_value(row.values).expect("Failed to deserialize balance snapshot");
            let balance_id = (details.journal_id, details.account_id, details.currency);
            let balance = AccountBalance::new(row.normal_balance_type, details);
            ret.insert(balance_id, balance);
        }
        Ok(ret)
    }

    #[instrument(name = "cala_ledger.balances.effective.list_for_account", skip_all)]
    pub(super) async fn list_for_account(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after_currency = after.map(|cursor| cursor.currency.code().to_string());

        let rows = sqlx::query!(
            r#"
            WITH account_balance_id AS (
              SELECT $2::uuid AS journal_id, $3::uuid AS account_id, a.normal_balance_type
              FROM cala_accounts a
              WHERE a.id = $3
            )
            SELECT
                h.values,
                account_balance_id.normal_balance_type as "normal_balance_type!: DebitOrCredit"
            FROM account_balance_id
            JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency)
                    journal_id, account_id, currency, values
                FROM cala_cumulative_effective_balances
                WHERE journal_id = account_balance_id.journal_id
                  AND account_id = account_balance_id.account_id
                  AND effective <= $4
                ORDER BY journal_id, account_id, currency, effective DESC, version DESC
            ) h ON TRUE
            WHERE ($5::text IS NULL OR h.currency > $5)
            ORDER BY h.currency ASC
            LIMIT $1
            "#,
            (first + 1) as i64,
            journal_id as JournalId,
            account_id as AccountId,
            date,
            after_currency.as_deref(),
        )
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(|row| {
                let details: BalanceSnapshot = serde_json::from_value(row.values)
                    .expect("Failed to deserialize balance snapshot");
                AccountBalance::new(row.normal_balance_type, details)
            })
            .collect::<Vec<_>>();
        let end_cursor = entities.last().map(AccountBalanceByCurrencyCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(name = "cala_ledger.balances.effective.list_for_accounts", skip_all)]
    pub(super) async fn list_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
            )
        } else {
            (None, None)
        };

        let rows = sqlx::query!(
            r#"
            WITH account_ids AS (
              SELECT DISTINCT account_id
              FROM UNNEST($2::uuid[]) AS v(account_id)
            ),
            account_balance_ids AS (
              SELECT $1::uuid AS journal_id, account_ids.account_id, a.normal_balance_type
              FROM account_ids
              JOIN cala_accounts a
              ON account_ids.account_id = a.id
            ),
            balances AS (
              SELECT
                  values,
                  normal_balance_type,
                  h.journal_id,
                  h.account_id,
                  h.currency
              FROM account_balance_ids
              JOIN LATERAL (
                  SELECT DISTINCT ON (journal_id, account_id, currency)
                      journal_id, account_id, currency, values
                  FROM cala_cumulative_effective_balances
                  WHERE journal_id = account_balance_ids.journal_id
                    AND account_id = account_balance_ids.account_id
                    AND effective <= $3
                  ORDER BY journal_id, account_id, currency, effective DESC, version DESC
              ) h ON TRUE
            )
            SELECT
                values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                h.currency
            FROM balances h
            WHERE (
                $4::uuid IS NULL
                OR (h.account_id, h.currency) > ($4::uuid, $5::text)
            )
            ORDER BY h.account_id ASC, h.currency ASC
            LIMIT $6
            "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
            date,
            after_account_id,
            after_currency.as_deref(),
            (first + 1) as i64,
        )
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(|row| {
                let details: BalanceSnapshot = serde_json::from_value(row.values)
                    .expect("Failed to deserialize balance snapshot");
                AccountBalance::new(row.normal_balance_type, details)
            })
            .collect::<Vec<_>>();
        let end_cursor = entities.last().map(AccountBalanceCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(name = "cala_ledger.balances.effective.find_range_all", skip_all)]
    pub(super) async fn find_range_all(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<BalanceRangeResult, BalanceError> {
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
              SELECT journal_id, account_id, currency, normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[])
                AS v(journal_id, account_id, currency)
              ) AS v
              JOIN cala_accounts a
              ON account_id = a.id
            ),
            first AS (
              SELECT
                true AS first, false AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $4
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            ),
            last AS (
              SELECT
                false AS first, true AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($5, NOW()::DATE)
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            )
            SELECT
                first, last, values, 
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM first
            UNION ALL
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
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
            let entry = ret.entry(balance_id).or_insert((None, 0, None, 0));
            if row.first.expect("first is not null") {
                entry.0 = Some(balance);
                entry.1 = row.all_time_version.expect("all_time_version") as u32;
            } else {
                entry.2 = Some(balance);
                entry.3 = row.all_time_version.expect("all_time_version") as u32;
            }
        }
        Ok(ret)
    }

    #[instrument(
        name = "cala_ledger.balances.effective.list_range_for_account",
        skip_all
    )]
    pub(super) async fn list_range_for_account(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after_currency = after.map(|cursor| cursor.currency.code().to_string());

        let rows = sqlx::query!(
            r#"
            WITH account_balance_id AS (
              SELECT $2::uuid AS journal_id, $3::uuid AS account_id, a.normal_balance_type
              FROM cala_accounts a
              WHERE a.id = $3
            ),
            balance_ids AS (
              SELECT h.journal_id, h.account_id, h.currency, account_balance_id.normal_balance_type
              FROM account_balance_id
              JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency)
                    journal_id, account_id, currency
                FROM cala_cumulative_effective_balances
                WHERE journal_id = account_balance_id.journal_id
                  AND account_id = account_balance_id.account_id
                  AND effective <= COALESCE($5, NOW()::DATE)
                ORDER BY journal_id, account_id, currency, effective DESC, version DESC
              ) h ON TRUE
              WHERE ($6::text IS NULL OR h.currency > $6)
              ORDER BY h.currency ASC
              LIMIT $1
            ),
            first AS (
              SELECT
                true AS first, false AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $4
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            ),
            last AS (
              SELECT
                false AS first, true AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($5, NOW()::DATE)
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            )
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM first
            UNION ALL
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM last"#,
            (first + 1) as i64,
            journal_id as JournalId,
            account_id as AccountId,
            from,
            until,
            after_currency.as_deref(),
        )
        .fetch_all(&self.pool)
        .await?;

        let mut ranges = HashMap::new();
        for row in rows {
            let values: serde_json::Value = row.values.expect("values is not null");
            let details: BalanceSnapshot =
                serde_json::from_value(values).expect("Failed to deserialize balance snapshot");
            let balance_id = (details.journal_id, details.account_id, details.currency);
            let balance = AccountBalance::new(row.normal_balance_type, details);
            let entry = ranges.entry(balance_id).or_insert((None, 0, None, 0));
            if row.first.expect("first is not null") {
                entry.0 = Some(balance);
                entry.1 = row.all_time_version.expect("all_time_version") as u32;
            } else {
                entry.2 = Some(balance);
                entry.3 = row.all_time_version.expect("all_time_version") as u32;
            }
        }

        let has_next_page = ranges.len() > first;
        let mut entities = Self::balance_ranges_from_snapshots(ranges);
        entities.truncate(first);
        let end_cursor = entities.last().map(AccountBalanceByCurrencyCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(
        name = "cala_ledger.balances.effective.list_range_for_accounts",
        skip_all
    )]
    pub(super) async fn list_range_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceCursor>, BalanceError>
    {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
            )
        } else {
            (None, None)
        };

        let rows = sqlx::query!(
            r#"
            WITH account_ids AS (
              SELECT DISTINCT account_id
              FROM UNNEST($2::uuid[]) AS v(account_id)
            ),
            account_balance_ids AS (
              SELECT $1::uuid AS journal_id, account_ids.account_id, a.normal_balance_type
              FROM account_ids
              JOIN cala_accounts a
              ON account_ids.account_id = a.id
            ),
            balance_ids AS (
              SELECT h.journal_id, h.account_id, h.currency, account_balance_ids.normal_balance_type
              FROM account_balance_ids
              JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency)
                    journal_id, account_id, currency
                FROM cala_cumulative_effective_balances
                WHERE journal_id = account_balance_ids.journal_id
                  AND account_id = account_balance_ids.account_id
                  AND effective <= COALESCE($4, NOW()::DATE)
                ORDER BY journal_id, account_id, currency, effective DESC, version DESC
              ) h ON TRUE
              WHERE (
                $5::uuid IS NULL
                OR (h.account_id, h.currency) > ($5::uuid, $6::text)
              )
              ORDER BY h.account_id ASC, h.currency ASC
              LIMIT $7
            ),
            first AS (
              SELECT
                true AS first, false AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $3
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            ),
            last AS (
              SELECT
                false AS first, true AS last, values,
                normal_balance_type,
                all_time_version,
                h.journal_id, h.account_id, h.currency
                FROM balance_ids
                JOIN LATERAL (
                    SELECT DISTINCT ON (journal_id, account_id, currency)
                        journal_id, account_id, currency, values, all_time_version
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($4, NOW()::DATE)
                    ORDER BY journal_id, account_id, currency, effective DESC, version DESC
                ) h ON TRUE
            )
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM first
            UNION ALL
            SELECT
                first, last, values,
                normal_balance_type as "normal_balance_type!: DebitOrCredit",
                all_time_version,
                journal_id as "journal_id: JournalId",
                account_id as "account_id: AccountId",
                currency
            FROM last"#,
            journal_id as JournalId,
            account_ids as &[AccountId],
            from,
            until,
            after_account_id,
            after_currency.as_deref(),
            (first + 1) as i64,
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
            let entry = ret.entry(balance_id).or_insert((None, 0, None, 0));
            if row.first.expect("first is not null") {
                entry.0 = Some(balance);
                entry.1 = row.all_time_version.expect("all_time_version") as u32;
            } else {
                entry.2 = Some(balance);
                entry.3 = row.all_time_version.expect("all_time_version") as u32;
            }
        }
        let has_next_page = ret.len() > first;
        let mut entities = Self::balance_ranges_from_snapshots(ret);
        entities.truncate(first);
        let end_cursor = entities.last().map(AccountBalanceCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    fn balance_ranges_from_snapshots(ranges: BalanceRangeResult) -> Vec<BalanceRange> {
        let mut ranges = ranges
            .into_iter()
            .filter_map(|(_, (start, start_version, end, end_version))| {
                end.map(|end| BalanceRange::new(start, end, end_version - start_version))
            })
            .collect::<Vec<_>>();

        ranges.sort_by_key(|range| {
            (
                range.close.details.journal_id,
                range.close.details.account_id,
                range.close.details.currency,
            )
        });

        ranges
    }

    #[instrument(
        name = "cala_ledger.balances.effective.find_for_update",
        skip(self, op)
    )]
    pub(super) async fn find_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): (Vec<AccountId>, Vec<&str>),
        effective: NaiveDate,
    ) -> Result<HashMap<(AccountId, Currency), EffectiveBalanceData<'_>>, BalanceError> {
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
              AND effective > $4
            RETURNING account_id, currency, effective, values
          ),
          values AS (
            SELECT 
              p.account_id,
              p.currency,
              b.values,
              b.all_time_version,
              b.effective
            FROM pairs p
            LEFT JOIN LATERAL (
              SELECT DISTINCT ON (account_id, currency)
                account_id,
                currency,
                values,
                all_time_version,
                effective
              FROM cala_cumulative_effective_balances
              WHERE journal_id = $1
                AND effective <= $4
                AND account_id = p.account_id
                AND currency = p.currency
              ORDER BY account_id, currency, all_time_version DESC
            ) b ON TRUE
          )
          SELECT
            v.account_id AS "account_id!: AccountId",
            v.currency AS "currency!",
            v.values AS "values?: serde_json::Value",
            v.all_time_version AS "all_time_version?: i32",
            v.effective AS "effective_date?: chrono::NaiveDate",
            COALESCE(
              jsonb_agg(
                jsonb_build_object('effective', d.effective, 'values', d.values)
              ) FILTER (WHERE d.values IS NOT NULL),
              '[]'::jsonb
            ) AS "deleted_values!: serde_json::Value"
          FROM values v
          LEFT JOIN delete_balances d
            ON v.account_id = d.account_id AND v.currency = d.currency
          GROUP BY v.account_id, v.currency, v.values, v.all_time_version, v.effective
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str],
            effective
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let last_snapshot = match (row.values, row.effective_date) {
                (Some(values), Some(effective_date)) => {
                    let snapshot = serde_json::from_value::<BalanceSnapshot>(values)
                        .expect("Failed to deserialize balance snapshot");
                    Some((effective_date, snapshot))
                }
                _ => None,
            };

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

    /// EC-set counterpart of [`Self::find_for_update`] used by the streaming
    /// rollup: identical delete-future / re-read-last logic, but keeps
    /// `eventually_consistent = TRUE` rows (the sets the inline poster path
    /// excludes).
    #[instrument(name = "effective_balance.find_ec_for_update", skip_all)]
    pub(super) async fn find_ec_for_update(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (account_ids, currencies): (Vec<AccountId>, Vec<&str>),
        effective: NaiveDate,
    ) -> Result<HashMap<(AccountId, Currency), EffectiveBalanceData<'_>>, BalanceError> {
        let rows = sqlx::query!(
            r#"
          WITH pairs AS (
            SELECT account_id, currency
            FROM (
              SELECT * FROM UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
            ) AS v
            JOIN cala_accounts a
            ON account_id = a.id
            WHERE eventually_consistent = TRUE
          ),
          delete_balances AS (
            DELETE FROM cala_cumulative_effective_balances
            WHERE journal_id = $1
              AND (account_id, currency) IN (SELECT account_id, currency FROM pairs)
              AND effective > $4
            RETURNING account_id, currency, effective, values
          ),
          values AS (
            SELECT
              p.account_id,
              p.currency,
              b.values,
              b.all_time_version,
              b.effective
            FROM pairs p
            LEFT JOIN LATERAL (
              SELECT DISTINCT ON (account_id, currency)
                account_id,
                currency,
                values,
                all_time_version,
                effective
              FROM cala_cumulative_effective_balances
              WHERE journal_id = $1
                AND effective <= $4
                AND account_id = p.account_id
                AND currency = p.currency
              ORDER BY account_id, currency, all_time_version DESC
            ) b ON TRUE
          )
          SELECT
            v.account_id AS "account_id!: AccountId",
            v.currency AS "currency!",
            v.values AS "values?: serde_json::Value",
            v.all_time_version AS "all_time_version?: i32",
            v.effective AS "effective_date?: chrono::NaiveDate",
            COALESCE(
              jsonb_agg(
                jsonb_build_object('effective', d.effective, 'values', d.values)
              ) FILTER (WHERE d.values IS NOT NULL),
              '[]'::jsonb
            ) AS "deleted_values!: serde_json::Value"
          FROM values v
          LEFT JOIN delete_balances d
            ON v.account_id = d.account_id AND v.currency = d.currency
          GROUP BY v.account_id, v.currency, v.values, v.all_time_version, v.effective
        "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies as &[&str],
            effective
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let last_snapshot = match (row.values, row.effective_date) {
                (Some(values), Some(effective_date)) => {
                    let snapshot = serde_json::from_value::<BalanceSnapshot>(values)
                        .expect("Failed to deserialize balance snapshot");
                    Some((effective_date, snapshot))
                }
                _ => None,
            };

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
        name = "cala_ledger.balances.effective.insert_new_snapshots",
        skip(self, op, new_balances)
    )]
    pub(crate) async fn insert_new_snapshots(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        new_balances: Vec<EffectiveBalanceSnapshot>,
    ) -> Result<(), BalanceError> {
        let mut journal_ids = Vec::with_capacity(new_balances.len());
        let mut account_ids = Vec::with_capacity(new_balances.len());
        let mut currencies = Vec::with_capacity(new_balances.len());
        let mut effectives = Vec::with_capacity(new_balances.len());
        let mut versions = Vec::with_capacity(new_balances.len());
        let mut all_time_versions = Vec::with_capacity(new_balances.len());
        let mut entry_ids = Vec::with_capacity(new_balances.len());
        let mut modified_timestamps = Vec::with_capacity(new_balances.len());
        let mut created_timestamps = Vec::with_capacity(new_balances.len());
        let mut values = Vec::with_capacity(new_balances.len());

        for balance in new_balances.iter() {
            journal_ids.push(journal_id);
            account_ids.push(balance.account_id);
            currencies.push(balance.currency.code());
            effectives.push(balance.effective);
            versions.push(balance.version as i32);
            all_time_versions.push(balance.all_time_version as i32);
            entry_ids.push(balance.entry_id);
            modified_timestamps.push(balance.modified_at);
            created_timestamps.push(balance.created_at);
            values
                .push(serde_json::to_value(balance).expect("Failed to serialize balance snapshot"));
        }

        sqlx::query!(
            r#"
            INSERT INTO cala_cumulative_effective_balances (
              journal_id, account_id, currency, effective, version, all_time_version, latest_entry_id, updated_at, created_at, values
            )
            SELECT * FROM UNNEST(
                $1::uuid[],
                $2::uuid[],
                $3::text[],
                $4::date[],
                $5::integer[],
                $6::integer[],
                $7::uuid[],
                $8::timestamptz[],
                $9::timestamptz[],
                $10::jsonb[]
            )
            "#,
            &journal_ids as &[JournalId],
            &account_ids as &[AccountId],
            &currencies[..] as &[&str],
            &effectives[..],
            &versions[..],
            &all_time_versions[..],
            &entry_ids as &[EntryId],
            &modified_timestamps[..],
            &created_timestamps[..],
            &values[..]
        )
        .execute(op.as_executor())
        .await?;

        self.publisher
            .publish_all(
                op,
                new_balances.into_iter().map(|balance| {
                    if balance.all_time_version == 1 {
                        OutboxEventPayload::EffectiveBalanceCreated { balance }
                    } else {
                        OutboxEventPayload::EffectiveBalanceUpdated { balance }
                    }
                }),
            )
            .await?;

        Ok(())
    }
}
