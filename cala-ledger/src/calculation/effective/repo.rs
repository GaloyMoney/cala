use chrono::{DateTime, NaiveDate, Utc};
use es_entity::errlanes::{lanes, Fault};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool, Row};
use std::collections::HashMap;

use cala_types::{
    calculation::{CalculationBalanceId, Window},
    primitives::{AccountId, BalanceId, CalculationId, EntryId, JournalId},
};

use crate::{
    balance::AccountBalance,
    effective::{
        cumulative::Checkpoints,
        delta::DateDelta,
        row::{effective_columns, AccountEffectiveRow},
    },
    error::CalaFault,
};

use super::CalculationSeriesCursor;

/// `(open, open all_time_version, close, close all_time_version)` per series.
pub(super) type RangeEnds =
    HashMap<CalculationBalanceId, (Option<AccountBalance>, u32, Option<AccountBalance>, u32)>;

/// The base effective columns plus the two series key columns, qualified
/// with the table alias `c`.
macro_rules! calculation_effective_columns {
    () => {
        concat!(effective_columns!(), ", c.calculation_id, c.dimensions")
    };
}

/// Joins the requested series `v` to their units and keeps the complete
/// ones: a series whose Global calculation or attachment is still being
/// backfilled is omitted.
macro_rules! complete_units {
    () => {
        "JOIN cala_calculations k ON k.id = v.calculation_id
         LEFT JOIN cala_calculation_attachments t
         ON t.calculation_id = v.calculation_id AND t.account_id = v.account_id
         WHERE k.backfill_complete AND COALESCE(t.backfill_complete, TRUE)"
    };
}

/// What one series read finds.
pub(super) enum Materialised {
    /// The unit is still being backfilled; its rows are a prefix of history.
    InProgress,
    /// No row at or before the date (or no such calculation).
    Absent,
    Present(Box<AccountBalance>),
}

/// One stored row with the target's normal balance type: the cumulative
/// balance of a series as of the end of one effective date.
#[derive(Debug, sqlx::FromRow)]
struct CalculationEffectiveRow {
    calculation_id: CalculationId,
    dimensions: Window,
    #[sqlx(flatten)]
    account_row: AccountEffectiveRow,
}

impl CalculationEffectiveRow {
    fn all_time_version(&self) -> u32 {
        self.account_row.all_time_version()
    }

    fn into_series(self) -> Result<(CalculationBalanceId, AccountBalance), Fault<lanes!(Fatal)>> {
        let balance = self.account_row.into_account_balance()?;
        let details = &balance.details;
        let id = CalculationBalanceId {
            journal_id: details.journal_id,
            calculation_id: self.calculation_id,
            account_id: details.account_id,
            currency: details.currency,
            dimensions: self.dimensions,
        };
        Ok((id, balance))
    }
}

/// One side of a range: the `first` flag marks the row just before the
/// range, otherwise the row at its end.
#[derive(Debug, sqlx::FromRow)]
struct RangeEndRow {
    first: bool,
    #[sqlx(flatten)]
    row: CalculationEffectiveRow,
}

/// The latest stored row of a series at or before `cutoff`.
#[derive(Debug, sqlx::FromRow)]
struct CheckpointRow {
    cutoff: NaiveDate,
    #[sqlx(flatten)]
    row: CalculationEffectiveRow,
}

/// Five aligned key arrays for the `UNNEST` of the `find_all*` reads.
struct KeyArrays {
    journal_ids: Vec<uuid::Uuid>,
    calculation_ids: Vec<uuid::Uuid>,
    account_ids: Vec<uuid::Uuid>,
    currencies: Vec<String>,
    dimensions: Vec<serde_json::Value>,
}

impl KeyArrays {
    fn new(ids: &[CalculationBalanceId]) -> Self {
        let mut arrays = Self {
            journal_ids: Vec::with_capacity(ids.len()),
            calculation_ids: Vec::with_capacity(ids.len()),
            account_ids: Vec::with_capacity(ids.len()),
            currencies: Vec::with_capacity(ids.len()),
            dimensions: Vec::with_capacity(ids.len()),
        };
        for id in ids {
            arrays.journal_ids.push(id.journal_id.into());
            arrays.calculation_ids.push(id.calculation_id.into());
            arrays.account_ids.push(id.account_id.into());
            arrays.currencies.push(id.currency.code().to_string());
            arrays.dimensions.push(id.dimensions.inner().clone());
        }
        arrays
    }
}

#[derive(Debug, Clone)]
pub(super) struct CalculationEffectiveBalanceRepo {
    pool: PgPool,
}

impl CalculationEffectiveBalanceRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.find_in_op",
        skip_all
    )]
    pub async fn find_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        id: &CalculationBalanceId,
        date: NaiveDate,
    ) -> Result<Materialised, CalaFault> {
        let row = sqlx::query(concat!(
            "WITH unit AS (
              SELECT NOT (k.backfill_complete AND COALESCE(t.backfill_complete, TRUE)) AS in_progress
              FROM cala_calculations k
              LEFT JOIN cala_calculation_attachments t
              ON t.calculation_id = k.id AND t.account_id = $3
              WHERE k.id = $2
            )
            SELECT unit.in_progress, ",
            calculation_effective_columns!(),
            ", a.normal_balance_type
            FROM unit
            LEFT JOIN LATERAL (
              SELECT *
              FROM cala_calculation_effective_balances
              WHERE NOT unit.in_progress
                AND journal_id = $1
                AND calculation_id = $2
                AND account_id = $3
                AND currency = $4
                AND dimensions = $5
                AND effective <= $6
              ORDER BY effective DESC
              LIMIT 1
            ) c ON TRUE
            LEFT JOIN cala_accounts a
            ON a.id = c.account_id"
        ))
        .bind(id.journal_id)
        .bind(id.calculation_id)
        .bind(id.account_id)
        .bind(id.currency.code())
        .bind(id.dimensions.inner())
        .bind(date)
        .fetch_optional(op.into_executor())
        .await?;

        let Some(row) = row else {
            return Ok(Materialised::Absent);
        };
        if row.try_get::<bool, _>("in_progress")? {
            return Ok(Materialised::InProgress);
        }
        if row
            .try_get::<Option<uuid::Uuid>, _>("calculation_id")?
            .is_none()
        {
            return Ok(Materialised::Absent);
        }
        let (_, balance) = CalculationEffectiveRow::from_row(&row)?.into_series()?;
        Ok(Materialised::Present(Box::new(balance)))
    }

    /// Whether the unit of `(calculation_id, account_id)` — the Global
    /// calculation, or the attachment — is still being backfilled.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.backfill_in_progress",
        skip(self)
    )]
    pub async fn backfill_in_progress(
        &self,
        calculation_id: CalculationId,
        account_id: AccountId,
    ) -> Result<bool, CalaFault> {
        let in_progress = sqlx::query_scalar::<_, bool>(
            "SELECT NOT (k.backfill_complete AND COALESCE(t.backfill_complete, TRUE))
             FROM cala_calculations k
             LEFT JOIN cala_calculation_attachments t
             ON t.calculation_id = k.id AND t.account_id = $2
             WHERE k.id = $1",
        )
        .bind(calculation_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(in_progress.unwrap_or(false))
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.find_all",
        skip_all
    )]
    pub async fn find_all(
        &self,
        ids: &[CalculationBalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<CalculationBalanceId, AccountBalance>, CalaFault> {
        let keys = KeyArrays::new(ids);
        let rows = sqlx::query_as::<_, CalculationEffectiveRow>(concat!(
            "WITH balance_ids AS (
              SELECT v.journal_id, v.calculation_id, v.account_id, v.currency, v.dimensions,
                     a.normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::uuid[], $4::text[], $5::jsonb[])
                AS v(journal_id, calculation_id, account_id, currency, dimensions)
              ) AS v
              JOIN cala_accounts a
              ON v.account_id = a.id
              ",
            complete_units!(),
            "
            )
            SELECT ",
            calculation_effective_columns!(),
            ", balance_ids.normal_balance_type
            FROM balance_ids
            JOIN LATERAL (
                SELECT *
                FROM cala_calculation_effective_balances
                WHERE journal_id = balance_ids.journal_id
                  AND calculation_id = balance_ids.calculation_id
                  AND account_id = balance_ids.account_id
                  AND currency = balance_ids.currency
                  AND dimensions = balance_ids.dimensions
                  AND effective <= $6
                ORDER BY effective DESC
                LIMIT 1
            ) c ON TRUE"
        ))
        .bind(&keys.journal_ids[..])
        .bind(&keys.calculation_ids[..])
        .bind(&keys.account_ids[..])
        .bind(&keys.currencies[..])
        .bind(&keys.dimensions[..])
        .bind(date)
        .fetch_all(&self.pool)
        .await?;

        let mut ret = HashMap::new();
        for row in rows {
            let (id, balance) = row.into_series()?;
            ret.insert(id, balance);
        }
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.find_range_all",
        skip_all
    )]
    pub async fn find_range_all(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[CalculationBalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<RangeEnds, CalaFault> {
        let keys = KeyArrays::new(ids);
        let rows = sqlx::query_as::<_, RangeEndRow>(concat!(
            "WITH balance_ids AS (
              SELECT v.journal_id, v.calculation_id, v.account_id, v.currency, v.dimensions,
                     a.normal_balance_type
              FROM (
                SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::uuid[], $4::text[], $5::jsonb[])
                AS v(journal_id, calculation_id, account_id, currency, dimensions)
              ) AS v
              JOIN cala_accounts a
              ON v.account_id = a.id
              ",
            complete_units!(),
            "
            ),
            first AS (
              SELECT true AS first, ",
            calculation_effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_calculation_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND calculation_id = balance_ids.calculation_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND dimensions = balance_ids.dimensions
                      AND effective < $6
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            ),
            last AS (
              SELECT false AS first, ",
            calculation_effective_columns!(),
            ", balance_ids.normal_balance_type
                FROM balance_ids
                JOIN LATERAL (
                    SELECT *
                    FROM cala_calculation_effective_balances
                    WHERE journal_id = balance_ids.journal_id
                      AND calculation_id = balance_ids.calculation_id
                      AND account_id = balance_ids.account_id
                      AND currency = balance_ids.currency
                      AND dimensions = balance_ids.dimensions
                      AND effective <= COALESCE($7, NOW()::DATE)
                    ORDER BY effective DESC
                    LIMIT 1
                ) c ON TRUE
            )
            SELECT * FROM first
            UNION ALL
            SELECT * FROM last"
        ))
        .bind(&keys.journal_ids[..])
        .bind(&keys.calculation_ids[..])
        .bind(&keys.account_ids[..])
        .bind(&keys.currencies[..])
        .bind(&keys.dimensions[..])
        .bind(from)
        .bind(until)
        .fetch_all(op.into_executor())
        .await?;

        let mut ret: RangeEnds = HashMap::new();
        for RangeEndRow { first, row } in rows {
            let all_time_version = row.all_time_version();
            let (id, balance) = row.into_series()?;
            let entry = ret.entry(id).or_insert((None, 0, None, 0));
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

    /// The latest row of each series at or before each of `cutoffs`, in one
    /// statement, as one [`Checkpoints`] map per series keyed by the
    /// series' base balance id.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.find_checkpoints",
        skip_all,
        fields(ids_count = ids.len(), cutoffs = cutoffs.len())
    )]
    pub async fn find_checkpoints(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        ids: &[CalculationBalanceId],
        cutoffs: &[NaiveDate],
    ) -> Result<HashMap<CalculationBalanceId, Checkpoints>, CalaFault> {
        let mut seen = std::collections::HashSet::with_capacity(ids.len());
        let unique: Vec<CalculationBalanceId> =
            ids.iter().filter(|id| seen.insert(*id)).cloned().collect();
        let keys = KeyArrays::new(&unique);
        let rows = sqlx::query_as::<_, CheckpointRow>(concat!(
            "WITH ids AS (
              SELECT v.journal_id, v.calculation_id, v.account_id, v.currency, v.dimensions,
                     a.normal_balance_type
              FROM UNNEST($1::uuid[], $2::uuid[], $3::uuid[], $4::text[], $5::jsonb[])
                AS v(journal_id, calculation_id, account_id, currency, dimensions)
              JOIN cala_accounts a ON a.id = v.account_id
              ",
            complete_units!(),
            "
            )
            SELECT d.cutoff, ",
            calculation_effective_columns!(),
            ", ids.normal_balance_type
            FROM ids
            CROSS JOIN UNNEST($6::date[]) AS d(cutoff)
            JOIN LATERAL (
              SELECT *
              FROM cala_calculation_effective_balances
              WHERE journal_id = ids.journal_id
                AND calculation_id = ids.calculation_id
                AND account_id = ids.account_id
                AND currency = ids.currency
                AND dimensions = ids.dimensions
                AND effective <= d.cutoff
              ORDER BY effective DESC
              LIMIT 1
            ) c ON TRUE"
        ))
        .bind(&keys.journal_ids[..])
        .bind(&keys.calculation_ids[..])
        .bind(&keys.account_ids[..])
        .bind(&keys.currencies[..])
        .bind(&keys.dimensions[..])
        .bind(cutoffs)
        .fetch_all(op.into_executor())
        .await?;

        let mut ret: HashMap<CalculationBalanceId, Checkpoints> = HashMap::new();
        for CheckpointRow { cutoff, row } in rows {
            let all_time_version = row.all_time_version();
            let (id, balance) = row.into_series()?;
            let base: BalanceId = (id.journal_id, id.account_id, id.currency);
            ret.entry(id)
                .or_default()
                .insert((cutoff, base), (balance, all_time_version));
        }
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.list_for_account",
        skip_all
    )]
    pub async fn list_for_account(
        &self,
        journal_id: JournalId,
        calculation_id: CalculationId,
        account_id: AccountId,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<CalculationSeriesCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<(Window, AccountBalance), CalculationSeriesCursor>,
        CalaFault,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = args;
        let (after_currency, after_dimensions) = match after {
            Some(cursor) => (
                Some(cursor.currency.code().to_string()),
                Some(cursor.dimensions.to_string()),
            ),
            None => (None, None),
        };

        let rows = sqlx::query_as::<_, CalculationEffectiveRow>(concat!(
            "WITH series AS (
              SELECT DISTINCT ON (c.currency, c.dimensions) ",
            calculation_effective_columns!(),
            ", a.normal_balance_type
              FROM cala_calculation_effective_balances c
              JOIN cala_accounts a
              ON c.account_id = a.id
              WHERE c.journal_id = $2
                AND c.calculation_id = $3
                AND c.account_id = $4
                AND c.effective <= $5
              ORDER BY c.currency, c.dimensions, c.effective DESC
            )
            SELECT * FROM series c
            WHERE $6::text IS NULL
               OR (c.currency, c.dimensions::text) > ($6, $7::jsonb::text)
            ORDER BY c.currency ASC, c.dimensions::text ASC
            LIMIT $1"
        ))
        .bind((first + 1) as i64)
        .bind(journal_id)
        .bind(calculation_id)
        .bind(account_id)
        .bind(date)
        .bind(after_currency.as_deref())
        .bind(after_dimensions.as_deref())
        .fetch_all(&self.pool)
        .await?;

        let has_next_page = rows.len() > first;
        let entities = rows
            .into_iter()
            .take(first)
            .map(|row| row.into_series().map(|(id, b)| (id.dimensions, b)))
            .collect::<Result<Vec<_>, Fault<lanes!(Fatal)>>>()?;
        let end_cursor = entities
            .last()
            .map(|(dimensions, balance)| CalculationSeriesCursor {
                currency: balance.details.currency,
                dimensions: dimensions.inner().clone(),
            });

        Ok(es_entity::PaginatedQueryRet::new(
            entities,
            has_next_page,
            end_cursor,
            first,
        ))
    }

    /// Applies a batch's per-`(series, date)` deltas to the stored cumulative
    /// balances, entirely inside Postgres: no balance row is read into the
    /// application.
    ///
    /// 1. **Ensure a row exists for every delta date**, seeded from the
    ///    series' latest earlier row (or zeros) with `version = 0`. Two new
    ///    dates of one series in the same batch both copy the same
    ///    predecessor; that is correct because step 2 then adds the earlier
    ///    date's delta to the later row.
    /// 2. **Add each delta to every row of its series on or after its date**,
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
    /// The calculation rollup is the sole writer of the table; the `UPDATE`
    /// row-locks the suffix until the operation commits.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation_effective_balance.apply_deltas_in_op",
        skip(self, op, deltas),
        fields(deltas_count = deltas.len())
    )]
    pub async fn apply_deltas_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        deltas: &[DateDelta<(CalculationId, Window)>],
        modified_at: DateTime<Utc>,
    ) -> Result<(), CalaFault> {
        if deltas.is_empty() {
            return Ok(());
        }
        let unassigned = EntryId::from(uuid::Uuid::nil());

        let calculation_ids: Vec<CalculationId> = deltas.iter().map(|d| d.series.0).collect();
        let account_ids: Vec<AccountId> = deltas.iter().map(|d| d.account_id).collect();
        let currencies: Vec<&str> = deltas.iter().map(|d| d.currency.code()).collect();
        let dimensions: Vec<serde_json::Value> =
            deltas.iter().map(|d| d.series.1.inner().clone()).collect();
        let effectives: Vec<NaiveDate> = deltas.iter().map(|d| d.effective).collect();

        sqlx::query!(
            r#"
            WITH d AS (
              SELECT DISTINCT calculation_id, account_id, currency, dimensions, effective
              FROM UNNEST($2::uuid[], $3::uuid[], $4::text[], $5::jsonb[], $6::date[])
                AS d(calculation_id, account_id, currency, dimensions, effective)
            )
            INSERT INTO cala_calculation_effective_balances (
              journal_id, calculation_id, account_id, currency, dimensions, effective,
              version, all_time_version, latest_entry_id,
              settled_dr_balance, settled_cr_balance, settled_entry_id, settled_modified_at,
              pending_dr_balance, pending_cr_balance, pending_entry_id, pending_modified_at,
              encumbrance_dr_balance, encumbrance_cr_balance, encumbrance_entry_id,
              encumbrance_modified_at,
              updated_at, created_at
            )
            SELECT
              $1::uuid, d.calculation_id, d.account_id, d.currency, d.dimensions, d.effective, 0,
              COALESCE(p.all_time_version, 0),
              COALESCE(p.latest_entry_id, $7::uuid),
              COALESCE(p.settled_dr_balance, 0), COALESCE(p.settled_cr_balance, 0),
              COALESCE(p.settled_entry_id, $7::uuid), COALESCE(p.settled_modified_at, $8),
              COALESCE(p.pending_dr_balance, 0), COALESCE(p.pending_cr_balance, 0),
              COALESCE(p.pending_entry_id, $7::uuid), COALESCE(p.pending_modified_at, $8),
              COALESCE(p.encumbrance_dr_balance, 0), COALESCE(p.encumbrance_cr_balance, 0),
              COALESCE(p.encumbrance_entry_id, $7::uuid), COALESCE(p.encumbrance_modified_at, $8),
              $8, COALESCE(p.created_at, $8)
            FROM d
            LEFT JOIN LATERAL (
              SELECT *
              FROM cala_calculation_effective_balances c
              WHERE c.journal_id = $1
                AND c.calculation_id = d.calculation_id
                AND c.account_id = d.account_id
                AND c.currency = d.currency
                AND c.dimensions = d.dimensions
                AND c.effective < d.effective
              ORDER BY c.effective DESC
              LIMIT 1
            ) p ON TRUE
            ON CONFLICT (journal_id, calculation_id, account_id, currency, dimensions, effective)
              DO NOTHING
            "#,
            journal_id as JournalId,
            &calculation_ids as &[CalculationId],
            &account_ids as &[AccountId],
            &currencies[..] as &[&str],
            &dimensions[..],
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

        sqlx::query!(
            r#"
            WITH d AS (
              SELECT * FROM UNNEST(
                $2::uuid[], $3::uuid[], $4::text[], $5::jsonb[], $6::date[],
                $7::numeric[], $8::numeric[], $9::numeric[], $10::numeric[],
                $11::numeric[], $12::numeric[],
                $13::int[], $14::uuid[], $15::uuid[], $16::uuid[], $17::uuid[]
              ) AS d(
                calculation_id, account_id, currency, dimensions, effective,
                settled_dr, settled_cr, pending_dr, pending_cr, encumbrance_dr, encumbrance_cr,
                n, last_entry_id, settled_entry_id, pending_entry_id, encumbrance_entry_id
              )
            ),
            s AS (
              SELECT
                c.calculation_id, c.account_id, c.currency, c.dimensions, c.effective,
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
                COALESCE(bool_or(d.settled_entry_id <> $18::uuid), FALSE) AS settled_touched,
                COALESCE(bool_or(d.pending_entry_id <> $18::uuid), FALSE) AS pending_touched,
                COALESCE(bool_or(d.encumbrance_entry_id <> $18::uuid), FALSE)
                  AS encumbrance_touched,
                (array_agg(d.settled_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.settled_entry_id <> $18::uuid
                    AND (c.effective = d.effective OR c.settled_entry_id = a.settled_entry_id)
                ))[1] AS settled_entry_id,
                (array_agg(d.pending_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.pending_entry_id <> $18::uuid
                    AND (c.effective = d.effective OR c.pending_entry_id = a.pending_entry_id)
                ))[1] AS pending_entry_id,
                (array_agg(d.encumbrance_entry_id ORDER BY d.effective DESC) FILTER (
                  WHERE d.encumbrance_entry_id <> $18::uuid
                    AND (c.effective = d.effective
                         OR c.encumbrance_entry_id = a.encumbrance_entry_id)
                ))[1] AS encumbrance_entry_id
              FROM d
              JOIN cala_calculation_effective_balances c
                ON c.journal_id = $1
               AND c.calculation_id = d.calculation_id
               AND c.account_id = d.account_id
               AND c.currency = d.currency
               AND c.dimensions = d.dimensions
               AND c.effective >= d.effective
              JOIN cala_calculation_effective_balances a
                ON a.journal_id = $1
               AND a.calculation_id = d.calculation_id
               AND a.account_id = d.account_id
               AND a.currency = d.currency
               AND a.dimensions = d.dimensions
               AND a.effective = d.effective
              GROUP BY c.calculation_id, c.account_id, c.currency, c.dimensions, c.effective
            )
            UPDATE cala_calculation_effective_balances c
            SET settled_dr_balance = c.settled_dr_balance + s.settled_dr,
                settled_cr_balance = c.settled_cr_balance + s.settled_cr,
                settled_entry_id = COALESCE(s.settled_entry_id, c.settled_entry_id),
                settled_modified_at =
                  CASE WHEN s.settled_touched THEN $19 ELSE c.settled_modified_at END,
                pending_dr_balance = c.pending_dr_balance + s.pending_dr,
                pending_cr_balance = c.pending_cr_balance + s.pending_cr,
                pending_entry_id = COALESCE(s.pending_entry_id, c.pending_entry_id),
                pending_modified_at =
                  CASE WHEN s.pending_touched THEN $19 ELSE c.pending_modified_at END,
                encumbrance_dr_balance = c.encumbrance_dr_balance + s.encumbrance_dr,
                encumbrance_cr_balance = c.encumbrance_cr_balance + s.encumbrance_cr,
                encumbrance_entry_id = COALESCE(s.encumbrance_entry_id, c.encumbrance_entry_id),
                encumbrance_modified_at =
                  CASE WHEN s.encumbrance_touched THEN $19 ELSE c.encumbrance_modified_at END,
                all_time_version = c.all_time_version + s.n_total,
                version = c.version + s.n_same_day,
                latest_entry_id = COALESCE(s.same_day_entry_id, c.latest_entry_id),
                updated_at = $19
            FROM s
            WHERE c.journal_id = $1
              AND c.calculation_id = s.calculation_id
              AND c.account_id = s.account_id
              AND c.currency = s.currency
              AND c.dimensions = s.dimensions
              AND c.effective = s.effective
            "#,
            journal_id as JournalId,
            &calculation_ids as &[CalculationId],
            &account_ids as &[AccountId],
            &currencies[..] as &[&str],
            &dimensions[..],
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

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use serde_json::json;

    use cala_types::{
        calculation::{CalculationBalanceId, CalculationScope, Window},
        entry::EntryValues,
        primitives::*,
    };
    use es_entity::errlanes::Fail;

    use super::CalculationEffectiveBalanceRepo;
    use crate::{
        account::NewAccount,
        calculation::{
            error::{CalculationBalanceNotFound, CalculationBalanceRejection},
            History, NewCalculation,
        },
        effective::delta::DeltaAccumulator,
        journal::NewJournal,
        CalaLedger, CalaLedgerConfig,
    };

    type Key = (CalculationId, Window);

    struct Fixture {
        cala: CalaLedger,
        repo: CalculationEffectiveBalanceRepo,
        journal_id: JournalId,
        calculation_id: CalculationId,
        account_id: AccountId,
    }

    async fn fixture() -> anyhow::Result<Fixture> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&std::env::var("PG_CON")?)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        let mut jobs =
            job::Jobs::init(job::JobSvcConfig::builder().pool(pool.clone()).build()?).await?;
        let cala = CalaLedger::init(
            CalaLedgerConfig::builder()
                .pool(pool.clone())
                .exec_migrations(false)
                .build()?,
            &mut jobs,
        )
        .await?;
        let journal_id = JournalId::new();
        cala.journals()
            .create(
                NewJournal::builder()
                    .id(journal_id)
                    .name(format!("calc-{journal_id}"))
                    .build(),
            )
            .await?;
        let account_id = AccountId::new();
        cala.accounts()
            .create(
                NewAccount::builder()
                    .id(account_id)
                    .name(format!("calc-{account_id}"))
                    .code(format!("calc-{account_id}"))
                    .build(),
            )
            .await?;
        let calculation_id = CalculationId::new();
        cala.calculations()
            .create(
                NewCalculation::builder()
                    .id(calculation_id)
                    .journal_id(journal_id)
                    .code(format!("calc-{calculation_id}"))
                    .name("apply deltas")
                    .scope(CalculationScope::Global)
                    .history(History::FromNow)
                    .build()?,
            )
            .await?;
        Ok(Fixture {
            repo: CalculationEffectiveBalanceRepo::new(&pool),
            cala,
            journal_id,
            calculation_id,
            account_id,
        })
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, d).unwrap()
    }

    fn entry(
        account_id: AccountId,
        sequence: u32,
        direction: DebitOrCredit,
        units: i64,
        currency: &str,
    ) -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            version: 1,
            transaction_id: TransactionId::new(),
            journal_id: JournalId::new(),
            account_id,
            entry_type: "TEST".to_string(),
            sequence,
            layer: Layer::Settled,
            currency: currency.parse().unwrap(),
            direction,
            units: Decimal::from(units),
            description: None,
            metadata: None,
        }
    }

    impl Fixture {
        fn key(&self, dimensions: serde_json::Value) -> Key {
            (self.calculation_id, Window::from(dimensions))
        }

        fn id(&self, dimensions: serde_json::Value, currency: &str) -> CalculationBalanceId {
            CalculationBalanceId {
                journal_id: self.journal_id,
                calculation_id: self.calculation_id,
                account_id: self.account_id,
                currency: currency.parse().unwrap(),
                dimensions: Window::from(dimensions),
            }
        }

        async fn apply(
            &self,
            batch: &[(Key, NaiveDate, usize, &EntryValues)],
        ) -> anyhow::Result<()> {
            let mut deltas = DeltaAccumulator::<Key>::new();
            for (key, date, tx_index, entry) in batch {
                deltas.push_keyed(key.clone(), self.account_id, *date, *tx_index, entry);
            }
            let mut op = self.cala.begin_operation().await?;
            self.repo
                .apply_deltas_in_op(&mut op, self.journal_id, &deltas.into_deltas(), Utc::now())
                .await?;
            op.commit().await?;
            Ok(())
        }

        /// `(effective, dr, cr, version, all_time_version, latest_entry_id)`
        /// per row of one series, oldest first.
        async fn rows(
            &self,
            dimensions: serde_json::Value,
            currency: &str,
        ) -> anyhow::Result<Vec<(NaiveDate, Decimal, Decimal, i32, i32, uuid::Uuid)>> {
            Ok(sqlx::query_as(
                "SELECT effective, settled_dr_balance, settled_cr_balance, version,
                        all_time_version, latest_entry_id
                 FROM cala_calculation_effective_balances
                 WHERE journal_id = $1 AND calculation_id = $2 AND account_id = $3
                   AND currency = $4 AND dimensions = $5
                 ORDER BY effective",
            )
            .bind(self.journal_id)
            .bind(self.calculation_id)
            .bind(self.account_id)
            .bind(currency)
            .bind(dimensions)
            .fetch_all(self.repo.pool())
            .await?)
        }
    }

    /// Writes the history used by both tests: one undimensioned USD series
    /// with same-day, backdated-before, backdated-between and later dates,
    /// then two USD dimension tuples and a BTC series in one batch.
    async fn write_history(f: &Fixture) -> anyhow::Result<Vec<EntryValues>> {
        use DebitOrCredit::*;
        let a = f.account_id;
        let e = [
            entry(a, 1, Credit, 10, "USD"),
            entry(a, 2, Credit, 5, "USD"),
            entry(a, 1, Credit, 3, "USD"),
            entry(a, 1, Debit, 4, "USD"),
            entry(a, 1, Credit, 2, "USD"),
            entry(a, 1, Credit, 1, "USD"),
            entry(a, 1, Credit, 7, "USD"),
            entry(a, 2, Credit, 9, "USD"),
            entry(a, 3, Credit, 11, "BTC"),
        ];
        let none = || f.key(json!({}));
        f.apply(&[(none(), day(10), 0, &e[0]), (none(), day(10), 0, &e[1])])
            .await?;
        f.apply(&[(none(), day(10), 0, &e[2])]).await?;
        f.apply(&[(none(), day(5), 0, &e[3])]).await?;
        f.apply(&[(none(), day(7), 0, &e[4]), (none(), day(20), 1, &e[5])])
            .await?;
        f.apply(&[
            (f.key(json!({"k": "a"})), day(10), 0, &e[6]),
            (f.key(json!({"k": "b"})), day(10), 0, &e[7]),
            (none(), day(10), 0, &e[8]),
        ])
        .await?;
        Ok(e.to_vec())
    }

    #[tokio::test]
    async fn apply_deltas_unit() -> anyhow::Result<()> {
        let f = fixture().await?;
        let e = write_history(&f).await?;
        let d = Decimal::from;

        assert_eq!(
            f.rows(json!({}), "USD").await?,
            vec![
                (day(5), d(4), d(0), 1, 1, e[3].id.into()),
                (day(7), d(4), d(2), 1, 2, e[4].id.into()),
                (day(10), d(4), d(20), 3, 5, e[2].id.into()),
                (day(20), d(4), d(21), 1, 6, e[5].id.into()),
            ]
        );
        assert_eq!(
            f.rows(json!({"k": "a"}), "USD").await?,
            vec![(day(10), d(0), d(7), 1, 1, e[6].id.into())]
        );
        assert_eq!(
            f.rows(json!({"k": "b"}), "USD").await?,
            vec![(day(10), d(0), d(9), 1, 1, e[7].id.into())]
        );
        assert_eq!(
            f.rows(json!({}), "BTC").await?,
            vec![(day(10), d(0), d(11), 1, 1, e[8].id.into())]
        );
        Ok(())
    }

    #[tokio::test]
    async fn reads() -> anyhow::Result<()> {
        let f = fixture().await?;
        write_history(&f).await?;
        let reads = f.cala.calculations().effective();
        let d = Decimal::from;
        let usd = f.id(json!({}), "USD");

        let on = reads.find_cumulative(&usd, day(10)).await?;
        assert_eq!(on.details.settled.cr_balance, d(20));
        assert_eq!(on.details.settled.dr_balance, d(4));
        assert_eq!(on.details.version, 3);
        let between = reads.find_cumulative(&usd, day(6)).await?;
        assert_eq!(between.details.settled.dr_balance, d(4));
        assert_eq!(between.details.settled.cr_balance, d(0));
        assert!(matches!(
            reads.find_cumulative(&usd, day(4)).await,
            Err(Fail::Rejected(CalculationBalanceRejection::NotFound(
                CalculationBalanceNotFound(id)
            ))) if id == usd
        ));

        let range = reads.find_in_range(&usd, day(6), Some(day(15))).await?;
        assert_eq!(range.open.details.settled.dr_balance, d(4));
        assert_eq!(range.open.details.settled.cr_balance, d(0));
        assert_eq!(range.period.details.settled.dr_balance, d(0));
        assert_eq!(range.period.details.settled.cr_balance, d(20));
        assert_eq!(range.period.details.version, 4);
        assert_eq!(range.close.details.settled.cr_balance, d(20));
        assert!(matches!(
            reads.find_in_range(&usd, day(1), Some(day(4))).await,
            Err(Fail::Rejected(CalculationBalanceRejection::NotFound(_)))
        ));

        let dim_a = f.id(json!({"k": "a"}), "USD");
        let wrong_dims = f.id(json!({"k": "z"}), "USD");
        let absent = CalculationBalanceId {
            account_id: AccountId::new(),
            ..usd.clone()
        };
        let ids = [usd.clone(), dim_a.clone(), wrong_dims.clone(), absent];
        let all = reads.find_all_cumulative(&ids, day(31)).await?;
        assert_eq!(all.len(), 2);
        assert_eq!(all[&usd].details.settled.cr_balance, d(21));
        assert_eq!(all[&dim_a].details.settled.cr_balance, d(7));

        let ranges = reads.find_all_in_range(&ids, day(1), Some(day(31))).await?;
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[&usd].period.details.settled.cr_balance, d(21));
        assert_eq!(ranges[&usd].period.details.version, 6);

        let first = reads
            .list_cumulative_for_account(
                f.journal_id,
                f.calculation_id,
                f.account_id,
                day(31),
                es_entity::PaginatedQueryArgs {
                    first: 2,
                    after: None,
                },
            )
            .await?;
        assert!(first.has_next_page());
        let second = reads
            .list_cumulative_for_account(
                f.journal_id,
                f.calculation_id,
                f.account_id,
                day(31),
                es_entity::PaginatedQueryArgs {
                    first: 2,
                    after: first.end_cursor().cloned(),
                },
            )
            .await?;
        assert!(!second.has_next_page());
        let listed: Vec<(String, serde_json::Value, Decimal)> = first
            .entities()
            .iter()
            .chain(second.entities().iter())
            .map(|(window, balance)| {
                (
                    balance.details.currency.code().to_string(),
                    window.inner().clone(),
                    balance.details.settled.cr_balance,
                )
            })
            .collect();
        assert_eq!(
            listed,
            vec![
                ("BTC".to_string(), json!({}), d(11)),
                ("USD".to_string(), json!({"k": "a"}), d(7)),
                ("USD".to_string(), json!({"k": "b"}), d(9)),
                ("USD".to_string(), json!({}), d(21)),
            ]
        );
        Ok(())
    }
}
