//! Backdating benchmark for the cumulative effective balance.
//!
//! Seeds one account set (and the sender/recipient accounts feeding it) with
//! `DATES` consecutive effective dates of history — the stored cumulative
//! rows directly, each standing for `ENTRIES_PER_DATE` entries — then posts
//! one transaction dated *before* the whole history, i.e. the incident shape.
//! Reports wall time, rows updated, WAL bytes, and how many balance rows the
//! application fetched (it must be none).
//!
//! The history is seeded straight into `cala_cumulative_effective_balances`
//! because one row per (pair, date) is the stored model: what a posting
//! costs depends on the number of later *dates*, and `ENTRIES_PER_DATE` only
//! feeds the `version` / `all_time_version` counters. The previous per-entry
//! model would have had to hold `DATES * ENTRIES_PER_DATE` rows per pair.
//!
//! Run against the dev database (`make start-deps`, then `PG_CON` set):
//!
//! ```text
//! cargo run --release -p cala-perf --example backdating
//! BENCH_DATES=1500 BENCH_ENTRIES_PER_DATE=2000 cargo run --release -p cala-perf --example backdating
//! ```

use chrono::{Duration, NaiveDate};
use sqlx::Row;
use tracing::field::{Field, Visit};
use tracing_subscriber::{layer::SubscriberExt, Layer};

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use cala_ledger::{account_set::*, primitives::BalanceRollup, tx_template::Params, *};
use cala_perf::{init_accounts, init_cala, init_journal, templates::simple_transfer};

const EFFECTIVE_TABLE: &str = "cala_cumulative_effective_balances";

/// What the application received from Postgres, per statement: sums the
/// `rows_returned` that sqlx reports on every statement it runs.
#[derive(Default)]
struct FetchStats {
    /// statement text -> (executions, rows returned to the application)
    statements: HashMap<String, (u64, u64)>,
}

struct FetchLayer(Arc<Mutex<FetchStats>>);

#[derive(Default)]
struct EventFields {
    statement: Option<String>,
    rows_returned: Option<u64>,
}

impl Visit for EventFields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "rows_returned" {
            self.rows_returned = Some(value);
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "rows_returned" {
            self.rows_returned = Some(value.max(0) as u64);
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "db.statement" {
            self.statement = Some(value.to_string());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" && self.statement.is_none() {
            self.statement = Some(format!("{value:?}"));
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for FetchLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut fields = EventFields::default();
        event.record(&mut fields);
        if let (Some(statement), Some(rows)) = (fields.statement, fields.rows_returned) {
            let key = statement.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut stats = self.0.lock().unwrap();
            let slot = stats.statements.entry(key).or_default();
            slot.0 += 1;
            slot.1 += rows;
        }
    }
}

fn env_or(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

async fn wal_lsn(pool: &sqlx::PgPool) -> anyhow::Result<String> {
    Ok(sqlx::query("SELECT pg_current_wal_lsn()::text")
        .fetch_one(pool)
        .await?
        .get(0))
}

async fn wal_bytes_since(pool: &sqlx::PgPool, start: &str) -> anyhow::Result<i64> {
    Ok(
        sqlx::query("SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), $1::pg_lsn)::bigint")
            .bind(start)
            .fetch_one(pool)
            .await?
            .get(0),
    )
}

/// Seed `dates` consecutive cumulative rows for one pair, starting at `start`.
async fn seed_pair(
    pool: &sqlx::PgPool,
    journal_id: JournalId,
    account_id: AccountId,
    start: NaiveDate,
    dates: i64,
    entries_per_date: i64,
    debit: bool,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO cala_cumulative_effective_balances (
          journal_id, account_id, currency, effective, version, all_time_version,
          latest_entry_id,
          settled_dr_balance, settled_cr_balance, settled_entry_id, settled_modified_at,
          pending_dr_balance, pending_cr_balance, pending_entry_id, pending_modified_at,
          encumbrance_dr_balance, encumbrance_cr_balance, encumbrance_entry_id,
          encumbrance_modified_at, updated_at, created_at
        )
        SELECT
          $1, $2, 'USD', $3::date + n::int, $5::int, ($5::int * (n + 1))::int,
          gen_random_uuid(),
          CASE WHEN $6 THEN 10 * $5::numeric * (n + 1) ELSE 0 END,
          CASE WHEN $6 THEN 0 ELSE 10 * $5::numeric * (n + 1) END,
          gen_random_uuid(), TIMESTAMPTZ '2000-01-01',
          0, 0, '00000000-0000-0000-0000-000000000000', TIMESTAMPTZ '2000-01-01',
          0, 0, '00000000-0000-0000-0000-000000000000', TIMESTAMPTZ '2000-01-01',
          TIMESTAMPTZ '2000-01-01', TIMESTAMPTZ '2000-01-01'
        FROM generate_series(0, $4::bigint - 1) AS n
        "#,
    )
    .bind(uuid::Uuid::from(journal_id))
    .bind(uuid::Uuid::from(account_id))
    .bind(start)
    .bind(dates)
    .bind(entries_per_date)
    .bind(debit)
    .execute(pool)
    .await?;
    Ok(())
}

async fn post_at(
    cala: &CalaLedger,
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
    effective: NaiveDate,
) -> anyhow::Result<std::time::Duration> {
    let mut params = Params::new();
    params.insert("journal_id", journal_id);
    params.insert("sender_id", sender);
    params.insert("recipient_id", recipient);
    params.insert("effective", effective);
    let started = Instant::now();
    cala.post_transaction(
        TransactionId::new(),
        simple_transfer::SIMPLE_TRANSFER_TEMPLATE_CODE,
        params,
    )
    .await?;
    Ok(started.elapsed())
}

struct Measured {
    wall: std::time::Duration,
    wal_bytes: i64,
    updated: i64,
    inserted: i64,
}

#[allow(clippy::too_many_arguments)]
async fn measure_post(
    cala: &CalaLedger,
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
    effective: NaiveDate,
    fetched: &Arc<Mutex<FetchStats>>,
) -> anyhow::Result<(Measured, FetchStats)> {
    let pool = cala.pool();
    let rows_before = journal_rows(pool, journal_id).await?;
    // Every transaction after this point has a larger xid, and the stored
    // `xmin` of a row is the xid of the transaction that last wrote it.
    let baseline_xid: i64 = sqlx::query("SELECT (txid_current() % 4294967296)::bigint")
        .fetch_one(pool)
        .await?
        .get(0);
    fetched.lock().unwrap().statements.clear();
    let lsn = wal_lsn(pool).await?;
    let wall = post_at(cala, journal_id, sender, recipient, effective).await?;
    let wal_bytes = wal_bytes_since(pool, &lsn).await?;
    // Snapshot what the application fetched *before* the measuring queries
    // below run, so they are not counted against the post.
    let stats = std::mem::take(&mut *fetched.lock().unwrap());
    let rows_after = journal_rows(pool, journal_id).await?;
    let written: i64 = sqlx::query(&format!(
        "SELECT count(*) FROM {EFFECTIVE_TABLE} \
         WHERE journal_id = $1 AND xmin::text::bigint > $2"
    ))
    .bind(uuid::Uuid::from(journal_id))
    .bind(baseline_xid)
    .fetch_one(pool)
    .await?
    .get(0);
    let inserted = rows_after - rows_before;
    Ok((
        Measured {
            wall,
            wal_bytes,
            // A new row is inserted and then has the delta applied to it, so
            // it is one of the written rows; the rest were existing rows.
            updated: written - inserted,
            inserted,
        },
        stats,
    ))
}

async fn journal_rows(pool: &sqlx::PgPool, journal_id: JournalId) -> anyhow::Result<i64> {
    Ok(sqlx::query(&format!(
        "SELECT count(*) FROM {EFFECTIVE_TABLE} WHERE journal_id = $1"
    ))
    .bind(uuid::Uuid::from(journal_id))
    .fetch_one(pool)
    .await?
    .get(0))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dates = env_or("BENCH_DATES", 1_500);
    let entries_per_date = env_or("BENCH_ENTRIES_PER_DATE", 2_000);

    let fetched = Arc::new(Mutex::new(FetchStats::default()));
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(FetchLayer(fetched.clone())),
    )?;

    let cala = init_cala().await?;
    simple_transfer::init(&cala).await?;
    let journal = init_journal(&cala, true).await?;
    let (sender, recipient) = init_accounts(&cala, false).await?;
    let set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("backdating benchmark set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;
    cala.account_sets()
        .add_member(set.id(), recipient.id())
        .await?;

    let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let pairs: Vec<(AccountId, bool)> = vec![
        (sender.id(), true),
        (recipient.id(), false),
        (AccountId::from(set.id()), false),
    ];
    for (account_id, debit) in &pairs {
        seed_pair(
            cala.pool(),
            journal.id(),
            *account_id,
            start,
            dates,
            entries_per_date,
            *debit,
        )
        .await?;
    }
    sqlx::query(&format!("ANALYZE {EFFECTIVE_TABLE}"))
        .execute(cala.pool())
        .await?;

    let table_rows: i64 = sqlx::query(&format!(
        "SELECT count(*) FROM {EFFECTIVE_TABLE} WHERE journal_id = $1"
    ))
    .bind(uuid::Uuid::from(journal.id()))
    .fetch_one(cala.pool())
    .await?
    .get(0);
    println!(
        "seeded {} pairs x {dates} dates ({table_rows} rows), {entries_per_date} entries/date \
         = {} entries per pair; the per-entry model would hold {} rows per pair",
        pairs.len(),
        dates * entries_per_date,
        dates * entries_per_date,
    );

    // The incident shape: a posting dated one day before the first row.
    let backdated = start - Duration::days(1);
    let (deep, deep_fetched) = measure_post(
        &cala,
        journal.id(),
        sender.id(),
        recipient.id(),
        backdated,
        &fetched,
    )
    .await?;

    // Control: the same posting dated after all history (the normal case).
    let latest = start + Duration::days(dates + 1);
    let (shallow, shallow_fetched) = measure_post(
        &cala,
        journal.id(),
        sender.id(),
        recipient.id(),
        latest,
        &fetched,
    )
    .await?;

    let balance_rows_fetched = |stats: &FetchStats| -> u64 {
        stats
            .statements
            .iter()
            .filter(|(sql, _)| sql.contains(EFFECTIVE_TABLE))
            .map(|(_, (_, rows))| *rows)
            .sum()
    };
    let total_rows_fetched =
        |stats: &FetchStats| -> u64 { stats.statements.values().map(|(_, rows)| *rows).sum() };

    println!();
    println!("| scenario | wall time | existing rows updated | new rows inserted | WAL bytes | balance rows fetched by app | all rows fetched by app |");
    println!("|---|---|---|---|---|---|---|");
    for (name, m, stats) in [
        ("backdated before all history", &deep, &deep_fetched),
        (
            "dated after all history (control)",
            &shallow,
            &shallow_fetched,
        ),
    ] {
        println!(
            "| {name} | {:.1} ms | {} | {} | {} | {} | {} |",
            m.wall.as_secs_f64() * 1000.0,
            m.updated,
            m.inserted,
            m.wal_bytes,
            balance_rows_fetched(stats),
            total_rows_fetched(stats),
        );
    }
    println!();
    // Per pair the statement pair inserts the delta's own date row (then
    // adds the delta to it) and adds the delta to every later date's row.
    let expected_updates = pairs.len() as i64 * dates;
    println!(
        "expected existing rows updated for the backdated post: pairs x later dates \
         = {} x {dates} = {expected_updates}; new rows inserted = {}",
        pairs.len(),
        pairs.len()
    );
    println!("statements run by the backdated post (executions, rows returned to the app):");
    for (sql, (runs, rows)) in &deep_fetched.statements {
        let shown: String = sql.chars().take(110).collect();
        println!("  {runs:>3} x  rows={rows:<5} {shown}");
    }

    assert_eq!(
        balance_rows_fetched(&deep_fetched),
        0,
        "the application must not fetch any balance row"
    );
    assert_eq!(
        deep.updated, expected_updates,
        "existing rows updated must equal pairs x later dates"
    );
    assert_eq!(deep.inserted, pairs.len() as i64);
    Ok(())
}
