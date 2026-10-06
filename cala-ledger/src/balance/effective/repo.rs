use crate::error::CalaFault;
use chrono::{DateTime, NaiveDate, Utc};
use es_entity::errlanes::{lanes, Fail, Fault, ResultExt};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::collections::HashMap;

use crate::balance::{
    account_balance::{AccountBalance, BalanceRange},
    cursor::{
        AccountBalanceByCurrencyCursor, AccountBalanceCursor, EffectiveBalancesModifiedCursor,
    },
    error::BalanceNotFound,
    snapshot::UNASSIGNED_ENTRY_ID,
};
use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot, EffectiveBalanceSnapshot},
    primitives::{AccountId, BalanceId, Currency, DebitOrCredit, EntryId, JournalId},
};

use super::delta::DateDelta;

type BalanceRangeResult =
    HashMap<BalanceId, (Option<AccountBalance>, u32, Option<AccountBalance>, u32)>;

/// The columns of one `cala_cumulative_effective_balances` row, qualified
/// with the table alias `c`. Every read query selects exactly this list so
/// that a single [`EffectiveRow`] mapper serves them all.
macro_rules! effective_columns {
    () => {
        "c.journal_id, c.account_id, c.currency, c.effective, c.version, \
         c.all_time_version, c.latest_entry_id, \
         c.settled_dr_balance, c.settled_cr_balance, c.settled_entry_id, c.settled_modified_at, \
         c.pending_dr_balance, c.pending_cr_balance, c.pending_entry_id, c.pending_modified_at, \
         c.encumbrance_dr_balance, c.encumbrance_cr_balance, c.encumbrance_entry_id, \
         c.encumbrance_modified_at, c.updated_at, c.created_at"
    };
}

/// One stored row: the cumulative balance of a pair as of the end of one
/// effective date.
#[derive(Debug, sqlx::FromRow)]
struct EffectiveRow {
    journal_id: JournalId,
    account_id: AccountId,
    currency: String,
    effective: NaiveDate,
    version: i32,
    all_time_version: i32,
    latest_entry_id: EntryId,
    settled_dr_balance: Decimal,
    settled_cr_balance: Decimal,
    settled_entry_id: EntryId,
    settled_modified_at: DateTime<Utc>,
    pending_dr_balance: Decimal,
    pending_cr_balance: Decimal,
    pending_entry_id: EntryId,
    pending_modified_at: DateTime<Utc>,
    encumbrance_dr_balance: Decimal,
    encumbrance_cr_balance: Decimal,
    encumbrance_entry_id: EntryId,
    encumbrance_modified_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl EffectiveRow {
    fn into_snapshot(self) -> Result<EffectiveBalanceSnapshot, Fault<lanes!(Fatal)>> {
        let currency: Currency = self
            .currency
            .parse()
            .classify::<crate::error::CouldNotDecodeCurrency>()?;
        Ok(EffectiveBalanceSnapshot {
            journal_id: self.journal_id,
            account_id: self.account_id,
            currency,
            effective: self.effective,
            version: self.version as u32,
            all_time_version: self.all_time_version as u32,
            created_at: self.created_at,
            modified_at: self.updated_at,
            entry_id: self.latest_entry_id,
            settled: BalanceAmount {
                dr_balance: self.settled_dr_balance,
                cr_balance: self.settled_cr_balance,
                entry_id: self.settled_entry_id,
                modified_at: self.settled_modified_at,
            },
            pending: BalanceAmount {
                dr_balance: self.pending_dr_balance,
                cr_balance: self.pending_cr_balance,
                entry_id: self.pending_entry_id,
                modified_at: self.pending_modified_at,
            },
            encumbrance: BalanceAmount {
                dr_balance: self.encumbrance_dr_balance,
                cr_balance: self.encumbrance_cr_balance,
                entry_id: self.encumbrance_entry_id,
                modified_at: self.encumbrance_modified_at,
            },
        })
    }

    fn into_balance_snapshot(self) -> Result<BalanceSnapshot, Fault<lanes!(Fatal)>> {
        let EffectiveBalanceSnapshot {
            journal_id,
            account_id,
            currency,
            version,
            created_at,
            modified_at,
            entry_id,
            settled,
            pending,
            encumbrance,
            ..
        } = self.into_snapshot()?;
        Ok(BalanceSnapshot {
            journal_id,
            account_id,
            currency,
            version,
            created_at,
            modified_at,
            entry_id,
            settled,
            pending,
            encumbrance,
        })
    }
}

/// A stored row together with the owning account's normal balance type.
#[derive(Debug, sqlx::FromRow)]
struct AccountEffectiveRow {
    #[sqlx(flatten)]
    row: EffectiveRow,
    normal_balance_type: DebitOrCredit,
}

impl AccountEffectiveRow {
    fn into_account_balance(self) -> Result<AccountBalance, Fault<lanes!(Fatal)>> {
        Ok(AccountBalance::new(
            self.normal_balance_type,
            self.row.into_balance_snapshot()?,
        ))
    }
}

/// One side of a balance range: the `first` flag marks the row just before
/// the range, otherwise the row at its end.
#[derive(Debug, sqlx::FromRow)]
struct RangeEndRow {
    first: bool,
    #[sqlx(flatten)]
    account_row: AccountEffectiveRow,
}

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
    ) -> Result<AccountBalance, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        self.find_in_op(&self.pool, journal_id, account_id, currency, date)
            .await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "effective_balance.find_in_op",
        skip_all
    )]
    pub async fn find_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, Fail<BalanceNotFound, lanes!(Transient, Fatal)>> {
        let row = sqlx::query_as::<_, AccountEffectiveRow>(concat!(
            "SELECT ",
            effective_columns!(),
            ", a.normal_balance_type
            FROM cala_cumulative_effective_balances c
            JOIN cala_accounts a
            ON c.account_id = a.id
            WHERE c.journal_id = $1
            AND c.account_id = $2
            AND c.currency = $3
            AND c.effective <= $4
            ORDER BY c.effective DESC
            LIMIT 1"
        ))
        .bind(journal_id)
        .bind(account_id)
        .bind(currency.code())
        .bind(date)
        .fetch_optional(op.into_executor())
        .await?;

        if let Some(row) = row {
            Ok(row.into_account_balance()?)
        } else {
            Err(BalanceNotFound(journal_id, account_id, currency).into())
        }
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "effective_balance.find_range",
        skip_all
    )]
    pub(super) async fn find_range(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<(Option<AccountBalance>, Option<AccountBalance>, u32), CalaFault> {
        let rows = sqlx::query_as::<_, RangeEndRow>(concat!(
            "WITH first AS (
                SELECT true AS first, ",
            effective_columns!(),
            ", a.normal_balance_type
                FROM cala_cumulative_effective_balances c
                JOIN cala_accounts a
                ON c.account_id = a.id
                WHERE c.journal_id = $1
                AND c.account_id = $2
                AND c.currency = $3
                AND c.effective < $4
                ORDER BY c.effective DESC
                LIMIT 1
            ),
            last AS (
                SELECT false AS first, ",
            effective_columns!(),
            ", a.normal_balance_type
                FROM cala_cumulative_effective_balances c
                JOIN cala_accounts a
                ON c.account_id = a.id
                WHERE c.journal_id = $1
                AND c.account_id = $2
                AND c.currency = $3
                AND c.effective <= COALESCE($5, NOW()::DATE)
                ORDER BY c.effective DESC
                LIMIT 1
            )
            SELECT * FROM first
            UNION ALL
            SELECT * FROM last"
        ))
        .bind(journal_id)
        .bind(account_id)
        .bind(currency.code())
        .bind(from)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;

        let mut first = None;
        let mut last = None;
        let mut first_version = 0;
        let mut last_version = 0;
        for RangeEndRow {
            first: is_first,
            account_row,
        } in rows
        {
            let all_time_version = account_row.row.all_time_version as u32;
            let balance = Some(account_row.into_account_balance().widen()?);
            if is_first {
                first = balance;
                first_version = all_time_version;
            } else {
                last = balance;
                last_version = all_time_version;
            }
        }
        Ok((first, last, last_version - first_version))
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.find_all",
        skip_all
    )]
    pub(super) async fn find_all(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<BalanceId, AccountBalance>, CalaFault> {
        let mut journal_ids = Vec::with_capacity(ids.len());
        let mut account_ids = Vec::with_capacity(ids.len());
        let mut currencies = Vec::with_capacity(ids.len());
        for (journal_id, account_id, currency) in ids {
            journal_ids.push(uuid::Uuid::from(journal_id));
            account_ids.push(uuid::Uuid::from(account_id));
            currencies.push(currency.code().to_string());
        }

        let rows = sqlx::query_as::<_, AccountEffectiveRow>(concat!(
            "WITH balance_ids AS (
              SELECT journal_id, account_id, currency, normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[])
                AS v(journal_id, account_id, currency)
              ) AS v
              JOIN cala_accounts a
              ON account_id = a.id
            )
            SELECT ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
            FROM balance_ids
            JOIN LATERAL (
                SELECT *
                FROM cala_cumulative_effective_balances
                WHERE journal_id = balance_ids.journal_id
                  AND account_id = balance_ids.account_id
                  AND currency = balance_ids.currency
                  AND effective <= $4
                ORDER BY effective DESC
                LIMIT 1
            ) c ON TRUE"
        ))
        .bind(&journal_ids[..])
        .bind(&account_ids[..])
        .bind(&currencies[..])
        .bind(date)
        .fetch_all(&self.pool)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let balance = row.into_account_balance().widen()?;
            let details = &balance.details;
            ret.insert(
                (details.journal_id, details.account_id, details.currency),
                balance,
            );
        }
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.list_for_account",
        skip_all
    )]
    pub(super) async fn list_for_account(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        CalaFault,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after_currency = after.map(|cursor| cursor.currency.code().to_string());

        let rows = sqlx::query_as::<_, AccountEffectiveRow>(concat!(
            "WITH account_balance_id AS (
              SELECT $2::uuid AS journal_id, $3::uuid AS account_id, a.normal_balance_type
              FROM cala_accounts a
              WHERE a.id = $3
            )
            SELECT ",
            effective_columns!(),
            ", account_balance_id.normal_balance_type
            FROM account_balance_id
            JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency) *
                FROM cala_cumulative_effective_balances
                WHERE journal_id = account_balance_id.journal_id
                  AND account_id = account_balance_id.account_id
                  AND effective <= $4
                ORDER BY journal_id, account_id, currency, effective DESC
            ) c ON TRUE
            WHERE ($5::text IS NULL OR c.currency > $5)
            ORDER BY c.currency ASC
            LIMIT $1"
        ))
        .bind((first + 1) as i64)
        .bind(journal_id)
        .bind(account_id)
        .bind(date)
        .bind(after_currency.as_deref())
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(AccountEffectiveRow::into_account_balance)
            .collect::<Result<Vec<_>, Fault<lanes!(Fatal)>>>()
            .widen()?;
        let end_cursor = entities.last().map(AccountBalanceByCurrencyCursor::from);

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.list_for_accounts",
        skip_all
    )]
    pub(super) async fn list_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, CalaFault> {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
            )
        } else {
            (None, None)
        };

        let rows = sqlx::query_as::<_, AccountEffectiveRow>(concat!(
            "WITH account_ids AS (
              SELECT DISTINCT account_id
              FROM UNNEST($2::uuid[]) AS v(account_id)
            ),
            account_balance_ids AS (
              SELECT $1::uuid AS journal_id, account_ids.account_id, a.normal_balance_type
              FROM account_ids
              JOIN cala_accounts a
              ON account_ids.account_id = a.id
            )
            SELECT ",
            effective_columns!(),
            ", account_balance_ids.normal_balance_type
            FROM account_balance_ids
            JOIN LATERAL (
                SELECT DISTINCT ON (journal_id, account_id, currency) *
                FROM cala_cumulative_effective_balances
                WHERE journal_id = account_balance_ids.journal_id
                  AND account_id = account_balance_ids.account_id
                  AND effective <= $3
                ORDER BY journal_id, account_id, currency, effective DESC
            ) c ON TRUE
            WHERE (
                $4::uuid IS NULL
                OR (c.account_id, c.currency) > ($4::uuid, $5::text)
            )
            ORDER BY c.account_id ASC, c.currency ASC
            LIMIT $6"
        ))
        .bind(journal_id)
        .bind(account_ids)
        .bind(date)
        .bind(after_account_id)
        .bind(after_currency.as_deref())
        .bind((first + 1) as i64)
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(AccountEffectiveRow::into_account_balance)
            .collect::<Result<Vec<_>, Fault<lanes!(Fatal)>>>()
            .widen()?;
        let end_cursor = entities.last().map(AccountBalanceCursor::from);

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    /// Backs [`super::EffectiveBalances::list_modified_since`]. Each stored
    /// row is already one `(account_id, currency, effective)` tuple's
    /// overall-latest cumulative snapshot, so this is a plain keyset-paginated
    /// scan: filtering on `updated_at >= since` is what makes it a "changed
    /// since" query rather than a full snapshot listing.
    ///
    /// Deliberately `updated_at`, not `created_at`: `created_at` is set once
    /// when a pair's first row is created and carried forward unchanged onto
    /// every later row of the pair — it does not mark per-row write time.
    /// `updated_at` is refreshed on every write that touches the row,
    /// including the later rows a backdated posting adjusts.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.list_modified_since",
        skip(self)
    )]
    pub(super) async fn list_modified_since(
        &self,
        journal_id: JournalId,
        since: DateTime<Utc>,
        args: es_entity::PaginatedQueryArgs<EffectiveBalancesModifiedCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<EffectiveBalanceSnapshot, EffectiveBalancesModifiedCursor>,
        CalaFault,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency, after_effective) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
                Some(after.effective),
            )
        } else {
            (None, None, None)
        };

        let rows = sqlx::query_as::<_, EffectiveRow>(concat!(
            "SELECT ",
            effective_columns!(),
            "
            FROM cala_cumulative_effective_balances c
            WHERE c.journal_id = $1
              AND c.updated_at >= $2
              AND (
                $3::uuid IS NULL
                OR (c.account_id, c.currency, c.effective) > ($3::uuid, $4::text, $5::date)
              )
            ORDER BY c.account_id, c.currency, c.effective
            LIMIT $6"
        ))
        .bind(journal_id)
        .bind(since)
        .bind(after_account_id)
        .bind(after_currency.as_deref())
        .bind(after_effective)
        .bind((first + 1) as i64)
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(EffectiveRow::into_snapshot)
            .collect::<Result<Vec<_>, Fault<lanes!(Fatal)>>>()
            .widen()?;
        let end_cursor = entities.last().map(EffectiveBalancesModifiedCursor::from);

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    /// Folds the first/last [`RangeEndRow`]s of a range query into
    /// `(start, start_all_time_version, end, end_all_time_version)` per pair.
    fn collect_range_ends(rows: Vec<RangeEndRow>) -> Result<BalanceRangeResult, CalaFault> {
        let mut ret: BalanceRangeResult = HashMap::new();
        for RangeEndRow { first, account_row } in rows {
            let all_time_version = account_row.row.all_time_version as u32;
            let balance = account_row.into_account_balance().widen()?;
            let details = &balance.details;
            let entry = ret
                .entry((details.journal_id, details.account_id, details.currency))
                .or_insert((None, 0, None, 0));
            if first {
                entry.0 = Some(balance);
                entry.1 = all_time_version;
            } else {
                entry.2 = Some(balance);
                entry.3 = all_time_version;
            }
        }
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.find_range_all",
        skip_all
    )]
    pub(super) async fn find_range_all(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<BalanceRangeResult, CalaFault> {
        let mut journal_ids = Vec::with_capacity(ids.len());
        let mut account_ids = Vec::with_capacity(ids.len());
        let mut currencies = Vec::with_capacity(ids.len());
        for (journal_id, account_id, currency) in ids {
            journal_ids.push(uuid::Uuid::from(journal_id));
            account_ids.push(uuid::Uuid::from(account_id));
            currencies.push(currency.code().to_string());
        }

        let rows = sqlx::query_as::<_, RangeEndRow>(concat!(
            "WITH balance_ids AS (
              SELECT journal_id, account_id, currency, normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[])
                AS v(journal_id, account_id, currency)
              ) AS v
              JOIN cala_accounts a
              ON account_id = a.id
            ),
            first AS (
              SELECT true AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $4
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            ),
            last AS (
              SELECT false AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($5, NOW()::DATE)
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            )
            SELECT * FROM first
            UNION ALL
            SELECT * FROM last"
        ))
        .bind(&journal_ids[..])
        .bind(&account_ids[..])
        .bind(&currencies[..])
        .bind(from)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;

        Self::collect_range_ends(rows)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
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
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceByCurrencyCursor>, CalaFault>
    {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let after_currency = after.map(|cursor| cursor.currency.code().to_string());

        let rows = sqlx::query_as::<_, RangeEndRow>(concat!(
            "WITH account_balance_id AS (
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
                ORDER BY journal_id, account_id, currency, effective DESC
              ) h ON TRUE
              WHERE ($6::text IS NULL OR h.currency > $6)
              ORDER BY h.currency ASC
              LIMIT $1
            ),
            first AS (
              SELECT true AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $4
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            ),
            last AS (
              SELECT false AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($5, NOW()::DATE)
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            )
            SELECT * FROM first
            UNION ALL
            SELECT * FROM last"
        ))
        .bind((first + 1) as i64)
        .bind(journal_id)
        .bind(account_id)
        .bind(from)
        .bind(until)
        .bind(after_currency.as_deref())
        .fetch_all(&self.pool)
        .await?;

        let ranges = Self::collect_range_ends(rows)?;
        let has_next_page = ranges.len() > first;
        let mut entities = Self::balance_ranges_from_snapshots(ranges);
        entities.truncate(first);
        let end_cursor = entities.last().map(AccountBalanceByCurrencyCursor::from);

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
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
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceCursor>, CalaFault> {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_account_id, after_currency) = if let Some(after) = after {
            (
                Some(uuid::Uuid::from(after.account_id)),
                Some(after.currency.code().to_string()),
            )
        } else {
            (None, None)
        };

        let rows = sqlx::query_as::<_, RangeEndRow>(concat!(
            "WITH account_ids AS (
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
                ORDER BY journal_id, account_id, currency, effective DESC
              ) h ON TRUE
              WHERE (
                $5::uuid IS NULL
                OR (h.account_id, h.currency) > ($5::uuid, $6::text)
              )
              ORDER BY h.account_id ASC, h.currency ASC
              LIMIT $7
            ),
            first AS (
              SELECT true AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective < $3
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            ),
            last AS (
              SELECT false AS first, ",
            effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_cumulative_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND effective <= COALESCE($4, NOW()::DATE)
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            )
            SELECT * FROM first
            UNION ALL
            SELECT * FROM last"
        ))
        .bind(journal_id)
        .bind(account_ids)
        .bind(from)
        .bind(until)
        .bind(after_account_id)
        .bind(after_currency.as_deref())
        .bind((first + 1) as i64)
        .fetch_all(&self.pool)
        .await?;

        let ret = Self::collect_range_ends(rows)?;
        let has_next_page = ret.len() > first;
        let mut entities = Self::balance_ranges_from_snapshots(ret);
        entities.truncate(first);
        let end_cursor = entities.last().map(AccountBalanceCursor::from);

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
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

    /// Applies a batch's per-`(pair, date)` deltas to the stored cumulative
    /// balances, entirely inside Postgres: no balance row is read into the
    /// application.
    ///
    /// 1. **Ensure a row exists for every delta date**, seeded from the
    ///    pair's latest earlier row (or zeros) with `version = 0`. Two new
    ///    dates of one pair in the same batch both copy the same
    ///    predecessor; that is correct because step 2 then adds the earlier
    ///    date's delta to the later row.
    /// 2. **Add each delta to every row of its pair on or after its date**,
    ///    once per row however many dates the batch spans. A row's
    ///    `all_time_version` grows by every applicable delta's entry count,
    ///    its `version` and `latest_entry_id` change only for deltas on its
    ///    own date.
    ///
    /// Per-layer `entry_id`: a delta's layer entry replaces the row's when
    /// the row is on the delta's own date, or when the row's current layer
    /// entry equals the one on the delta-date row before the update — i.e.
    /// no pre-existing entry of that layer lies in between. With several
    /// delta dates only the latest applicable one counts.
    ///
    /// The caller must hold the same per-pair serialization the previous
    /// delete-and-reinsert relied on (the EC rollup's singleton job, or the
    /// poster's balance locks); the `UPDATE` row-locks the suffix until the
    /// operation commits.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.balances.effective.apply_deltas_in_op",
        skip(self, op, deltas),
        fields(deltas_count = deltas.len())
    )]
    pub(super) async fn apply_deltas_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        deltas: &[DateDelta],
        modified_at: DateTime<Utc>,
    ) -> Result<(), CalaFault> {
        if deltas.is_empty() {
            return Ok(());
        }
        let unassigned = EntryId::from(UNASSIGNED_ENTRY_ID);

        let account_ids: Vec<AccountId> = deltas.iter().map(|d| d.account_id).collect();
        let currencies: Vec<&str> = deltas.iter().map(|d| d.currency.code()).collect();
        let effectives: Vec<NaiveDate> = deltas.iter().map(|d| d.effective).collect();

        sqlx::query!(
            r#"
            WITH d AS (
              SELECT DISTINCT account_id, currency, effective
              FROM UNNEST($2::uuid[], $3::text[], $4::date[])
                AS d(account_id, currency, effective)
            )
            INSERT INTO cala_cumulative_effective_balances (
              journal_id, account_id, currency, effective, version, all_time_version,
              latest_entry_id,
              settled_dr_balance, settled_cr_balance, settled_entry_id, settled_modified_at,
              pending_dr_balance, pending_cr_balance, pending_entry_id, pending_modified_at,
              encumbrance_dr_balance, encumbrance_cr_balance, encumbrance_entry_id,
              encumbrance_modified_at,
              updated_at, created_at
            )
            SELECT
              $1::uuid, d.account_id, d.currency, d.effective, 0,
              COALESCE(p.all_time_version, 0),
              COALESCE(p.latest_entry_id, $5::uuid),
              COALESCE(p.settled_dr_balance, 0), COALESCE(p.settled_cr_balance, 0),
              COALESCE(p.settled_entry_id, $5::uuid), COALESCE(p.settled_modified_at, $6),
              COALESCE(p.pending_dr_balance, 0), COALESCE(p.pending_cr_balance, 0),
              COALESCE(p.pending_entry_id, $5::uuid), COALESCE(p.pending_modified_at, $6),
              COALESCE(p.encumbrance_dr_balance, 0), COALESCE(p.encumbrance_cr_balance, 0),
              COALESCE(p.encumbrance_entry_id, $5::uuid), COALESCE(p.encumbrance_modified_at, $6),
              $6, COALESCE(p.created_at, $6)
            FROM d
            LEFT JOIN LATERAL (
              SELECT *
              FROM cala_cumulative_effective_balances c
              WHERE c.journal_id = $1
                AND c.account_id = d.account_id
                AND c.currency = d.currency
                AND c.effective < d.effective
              ORDER BY c.effective DESC
              LIMIT 1
            ) p ON TRUE
            ON CONFLICT (journal_id, account_id, currency, effective) DO NOTHING
            "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies[..] as &[&str],
            &effectives[..],
            unassigned as EntryId,
            modified_at,
        )
        .execute(op.as_executor())
        .await?;

        let n: Vec<i32> = deltas.iter().map(|d| d.entries as i32).collect();
        let last_entry_ids: Vec<EntryId> = deltas.iter().map(|d| d.last_entry_id).collect();
        let settled_dr: Vec<Decimal> = deltas.iter().map(|d| d.settled.dr_balance).collect();
        let settled_cr: Vec<Decimal> = deltas.iter().map(|d| d.settled.cr_balance).collect();
        let settled_entry_ids: Vec<EntryId> = deltas.iter().map(|d| d.settled.entry_id).collect();
        let pending_dr: Vec<Decimal> = deltas.iter().map(|d| d.pending.dr_balance).collect();
        let pending_cr: Vec<Decimal> = deltas.iter().map(|d| d.pending.cr_balance).collect();
        let pending_entry_ids: Vec<EntryId> = deltas.iter().map(|d| d.pending.entry_id).collect();
        let encumbrance_dr: Vec<Decimal> =
            deltas.iter().map(|d| d.encumbrance.dr_balance).collect();
        let encumbrance_cr: Vec<Decimal> =
            deltas.iter().map(|d| d.encumbrance.cr_balance).collect();
        let encumbrance_entry_ids: Vec<EntryId> =
            deltas.iter().map(|d| d.encumbrance.entry_id).collect();

        // The nil UUID ($16) marks "this delta has no entry in the layer".
        sqlx::query!(
            r#"
            WITH d AS (
              SELECT * FROM UNNEST(
                $2::uuid[], $3::text[], $4::date[],
                $5::numeric[], $6::numeric[], $7::numeric[], $8::numeric[],
                $9::numeric[], $10::numeric[],
                $11::int[], $12::uuid[], $13::uuid[], $14::uuid[], $15::uuid[]
              ) AS d(
                account_id, currency, effective,
                settled_dr, settled_cr, pending_dr, pending_cr, encumbrance_dr, encumbrance_cr,
                n, last_entry_id, settled_entry_id, pending_entry_id, encumbrance_entry_id
              )
            ),
            s AS (
              SELECT
                c.account_id, c.currency, c.effective,
                SUM(d.settled_dr) AS settled_dr,
                SUM(d.settled_cr) AS settled_cr,
                SUM(d.pending_dr) AS pending_dr,
                SUM(d.pending_cr) AS pending_cr,
                SUM(d.encumbrance_dr) AS encumbrance_dr,
                SUM(d.encumbrance_cr) AS encumbrance_cr,
                SUM(d.n)::int AS n_total,
                COALESCE(SUM(d.n) FILTER (WHERE d.effective = c.effective), 0)::int
                  AS n_same_day,
                (array_agg(d.last_entry_id) FILTER (WHERE d.effective = c.effective))[1]
                  AS same_day_entry_id,
                COALESCE(bool_or(d.settled_entry_id <> $16::uuid), FALSE) AS settled_touched,
                COALESCE(bool_or(d.pending_entry_id <> $16::uuid), FALSE) AS pending_touched,
                COALESCE(bool_or(d.encumbrance_entry_id <> $16::uuid), FALSE)
                  AS encumbrance_touched,
                (array_agg(d.settled_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.settled_entry_id <> $16::uuid
                    AND (c.effective = d.effective OR c.settled_entry_id = a.settled_entry_id)
                ))[1] AS settled_entry_id,
                (array_agg(d.pending_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.pending_entry_id <> $16::uuid
                    AND (c.effective = d.effective OR c.pending_entry_id = a.pending_entry_id)
                ))[1] AS pending_entry_id,
                (array_agg(d.encumbrance_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.encumbrance_entry_id <> $16::uuid
                    AND (c.effective = d.effective
                         OR c.encumbrance_entry_id = a.encumbrance_entry_id)
                ))[1] AS encumbrance_entry_id
              FROM d
              JOIN cala_cumulative_effective_balances c
                ON c.journal_id = $1
               AND c.account_id = d.account_id
               AND c.currency = d.currency
               AND c.effective >= d.effective
              JOIN cala_cumulative_effective_balances a
                ON a.journal_id = $1
               AND a.account_id = d.account_id
               AND a.currency = d.currency
               AND a.effective = d.effective
              GROUP BY c.account_id, c.currency, c.effective
            )
            UPDATE cala_cumulative_effective_balances c
            SET settled_dr_balance = c.settled_dr_balance + s.settled_dr,
                settled_cr_balance = c.settled_cr_balance + s.settled_cr,
                settled_entry_id = COALESCE(s.settled_entry_id, c.settled_entry_id),
                settled_modified_at =
                  CASE WHEN s.settled_touched THEN $17 ELSE c.settled_modified_at END,
                pending_dr_balance = c.pending_dr_balance + s.pending_dr,
                pending_cr_balance = c.pending_cr_balance + s.pending_cr,
                pending_entry_id = COALESCE(s.pending_entry_id, c.pending_entry_id),
                pending_modified_at =
                  CASE WHEN s.pending_touched THEN $17 ELSE c.pending_modified_at END,
                encumbrance_dr_balance = c.encumbrance_dr_balance + s.encumbrance_dr,
                encumbrance_cr_balance = c.encumbrance_cr_balance + s.encumbrance_cr,
                encumbrance_entry_id = COALESCE(s.encumbrance_entry_id, c.encumbrance_entry_id),
                encumbrance_modified_at =
                  CASE WHEN s.encumbrance_touched THEN $17 ELSE c.encumbrance_modified_at END,
                all_time_version = c.all_time_version + s.n_total,
                version = c.version + s.n_same_day,
                latest_entry_id = COALESCE(s.same_day_entry_id, c.latest_entry_id),
                updated_at = $17
            FROM s
            WHERE c.journal_id = $1
              AND c.account_id = s.account_id
              AND c.currency = s.currency
              AND c.effective = s.effective
            "#,
            journal_id as JournalId,
            &account_ids as &[AccountId],
            &currencies[..] as &[&str],
            &effectives[..],
            &settled_dr[..],
            &settled_cr[..],
            &pending_dr[..],
            &pending_cr[..],
            &encumbrance_dr[..],
            &encumbrance_cr[..],
            &n[..],
            &last_entry_ids as &[EntryId],
            &settled_entry_ids as &[EntryId],
            &pending_entry_ids as &[EntryId],
            &encumbrance_entry_ids as &[EntryId],
            unassigned as EntryId,
            modified_at,
        )
        .execute(op.as_executor())
        .await?;

        Ok(())
    }
}
