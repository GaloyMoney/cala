//! Differential test: the set-based SQL rule against the old in-memory fold.
//!
//! Random histories (several pairs, mixed dates, batches of mixed-date
//! transactions, all three layers) are applied twice:
//!
//! * through the old per-entry fold (`fold_oracle::EffectiveBalanceData`),
//!   modelling the old table as a `Vec` of per-entry rows per pair, and
//! * through the new `EffectiveBalanceRepo::apply_deltas_in_op` against a
//!   real Postgres.
//!
//! After every batch, for each `(pair, date)` the old last-version row must
//! equal the new row in the six amounts, `version`, `all_time_version`, the
//! row's `entry_id` and the per-layer entry ids.
//!
//! Needs `PG_CON` (the repo's dev Postgres); each case uses fresh random
//! journal/account ids, so cases and other suites do not interfere.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use rand::{rngs::StdRng, RngExt, SeedableRng};
use rust_decimal::Decimal;

use std::collections::HashMap;

use cala_types::{
    balance::{BalanceSnapshot, EffectiveBalanceSnapshot},
    entry::EntryValues,
    primitives::*,
};

use super::{
    delta::DeltaAccumulator,
    fold_oracle::{EffectiveBalanceData, SnapshotOrEntry},
    repo::EffectiveBalanceRepo,
};
use crate::balance::cursor::EffectiveBalancesModifiedCursor;

type Pair = (AccountId, Currency);

struct OracleTxn {
    effective: NaiveDate,
    created_at: DateTime<Utc>,
    entries: Vec<(Vec<AccountId>, EntryValues)>,
}

fn to_balance_snapshot(row: &EffectiveBalanceSnapshot) -> BalanceSnapshot {
    BalanceSnapshot {
        journal_id: row.journal_id,
        account_id: row.account_id,
        currency: row.currency,
        version: row.version,
        created_at: row.created_at,
        modified_at: row.modified_at,
        entry_id: row.entry_id,
        settled: row.settled.clone(),
        pending: row.pending.clone(),
        encumbrance: row.encumbrance.clone(),
    }
}

fn random_history(
    rng: &mut StdRng,
    journal_id: JournalId,
    accounts: &[AccountId],
    base_date: NaiveDate,
    base_time: DateTime<Utc>,
) -> Vec<Vec<OracleTxn>> {
    let batches = rng.random_range(1..=7);
    let span_days = rng.random_range(1..=9);
    let mut clock = 0i64;
    (0..batches)
        .map(|_| {
            (0..rng.random_range(1..=4))
                .map(|_| {
                    clock += 1;
                    let transaction_id = TransactionId::new();
                    let entries = (0..rng.random_range(1..=4u32))
                        .map(|sequence| {
                            let layer = match rng.random_range(0..3) {
                                0 => Layer::Settled,
                                1 => Layer::Pending,
                                _ => Layer::Encumbrance,
                            };
                            let direction = if rng.random_bool(0.5) {
                                DebitOrCredit::Debit
                            } else {
                                DebitOrCredit::Credit
                            };
                            let currency = if rng.random_bool(0.7) {
                                Currency::USD
                            } else {
                                Currency::BTC
                            };
                            // Non-empty subset of accounts: the entry's own
                            // account plus any ancestor sets it fans into.
                            let mut targets: Vec<AccountId> = accounts
                                .iter()
                                .copied()
                                .filter(|_| rng.random_bool(0.5))
                                .collect();
                            if targets.is_empty() {
                                targets.push(accounts[rng.random_range(0..accounts.len())]);
                            }
                            let entry = EntryValues {
                                id: EntryId::new(),
                                version: 1,
                                transaction_id,
                                journal_id,
                                account_id: targets[0],
                                entry_type: "ENTRY".to_string(),
                                sequence,
                                layer,
                                units: Decimal::from(rng.random_range(1..=100)),
                                currency,
                                direction,
                                description: None,
                                metadata: None,
                            };
                            (targets, entry)
                        })
                        .collect();
                    OracleTxn {
                        effective: base_date + Duration::days(rng.random_range(0..span_days)),
                        created_at: base_time + Duration::seconds(clock),
                        entries,
                    }
                })
                .collect()
        })
        .collect()
}

/// Apply one batch through the old fold, as the old `find_for_update` +
/// `re_calculate_snapshots` + `insert_new_snapshots` did: per pair, take the
/// row at-or-before the pair's earliest date as the anchor, "delete" every
/// later row, replay them with the batch's entries, and store the result.
fn apply_batch_with_oracle(
    model: &mut HashMap<Pair, Vec<EffectiveBalanceSnapshot>>,
    journal_id: JournalId,
    batch: &[OracleTxn],
) {
    type PairEntries<'a> = Vec<(usize, NaiveDate, DateTime<Utc>, &'a EntryValues)>;
    let mut per_pair: HashMap<Pair, PairEntries<'_>> = HashMap::new();
    for (tx_index, tx) in batch.iter().enumerate() {
        for (targets, entry) in &tx.entries {
            for &account_id in targets {
                per_pair
                    .entry((account_id, entry.currency))
                    .or_default()
                    .push((tx_index, tx.effective, tx.created_at, entry));
            }
        }
    }
    let rewritten_at = batch.iter().map(|tx| tx.created_at).max().unwrap();

    for ((account_id, currency), entries) in per_pair {
        let earliest = entries.iter().map(|(_, date, _, _)| *date).min().unwrap();
        let rows = model.entry((account_id, currency)).or_default();
        let (kept, deleted): (Vec<_>, Vec<_>) =
            rows.drain(..).partition(|r| r.effective <= earliest);
        let anchor = kept.iter().max_by_key(|r| r.all_time_version);
        let last_snapshot = anchor.map(|r| (r.effective, to_balance_snapshot(r)));
        let latest_all_time_version = anchor.map(|r| r.all_time_version).unwrap_or(0);
        let updates = deleted
            .iter()
            .map(|r| SnapshotOrEntry::Snapshot {
                effective: r.effective,
                values: to_balance_snapshot(r),
            })
            .collect();
        let mut data = EffectiveBalanceData::new(
            account_id,
            currency,
            last_snapshot,
            latest_all_time_version,
            updates,
        );
        for (tx_index, effective, created_at, entry) in entries {
            data.push(effective, tx_index, created_at, entry);
        }
        data.re_calculate_snapshots(rewritten_at);
        let mut rebuilt = kept;
        rebuilt.extend(data.into_snapshots(journal_id));
        *rows = rebuilt;
    }
}

async fn apply_batch_with_sql(
    repo: &EffectiveBalanceRepo,
    pool: &sqlx::PgPool,
    journal_id: JournalId,
    batch: &[OracleTxn],
) -> anyhow::Result<()> {
    let mut deltas = DeltaAccumulator::new();
    for (tx_index, tx) in batch.iter().enumerate() {
        for (targets, entry) in &tx.entries {
            for &account_id in targets {
                deltas.push(account_id, tx.effective, tx_index, entry);
            }
        }
    }
    let modified_at = batch.iter().map(|tx| tx.created_at).max().unwrap();
    let mut db = pool.begin().await?;
    repo.apply_deltas_in_op(&mut db, journal_id, &deltas.into_deltas(), modified_at)
        .await
        .map_err(|e| anyhow::anyhow!("apply_deltas_in_op failed: {e:?}"))?;
    db.commit().await?;
    Ok(())
}

async fn stored_rows(
    repo: &EffectiveBalanceRepo,
    journal_id: JournalId,
) -> anyhow::Result<Vec<EffectiveBalanceSnapshot>> {
    let page = repo
        .list_modified_since(
            journal_id,
            Utc.timestamp_opt(0, 0).unwrap(),
            es_entity::PaginatedQueryArgs::<EffectiveBalancesModifiedCursor> {
                first: 100_000,
                after: None,
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("list_modified_since failed: {e:?}"))?;
    Ok(page.entities().to_vec())
}

/// The old table kept every per-entry row; readers only ever used the last
/// one of each date, which is what the new table stores.
fn last_row_per_date(
    model: &HashMap<Pair, Vec<EffectiveBalanceSnapshot>>,
) -> Vec<EffectiveBalanceSnapshot> {
    let mut out = Vec::new();
    for rows in model.values() {
        let mut by_date: HashMap<NaiveDate, &EffectiveBalanceSnapshot> = HashMap::new();
        for row in rows {
            let slot = by_date.entry(row.effective).or_insert(row);
            if row.version > slot.version {
                *slot = row;
            }
        }
        out.extend(by_date.into_values().cloned());
    }
    out.sort_by_key(|r| (r.account_id, r.currency.code().to_string(), r.effective));
    out
}

/// `(what differs, description)` for every disagreement.
fn diff_rows(
    expected: &[EffectiveBalanceSnapshot],
    actual: &[EffectiveBalanceSnapshot],
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut actual_sorted = actual.to_vec();
    actual_sorted.sort_by_key(|r| (r.account_id, r.currency.code().to_string(), r.effective));
    if expected.len() != actual_sorted.len() {
        out.push((
            "row-set",
            format!(
                "expected {} rows, got {}",
                expected.len(),
                actual_sorted.len()
            ),
        ));
        return out;
    }
    for (e, a) in expected.iter().zip(&actual_sorted) {
        let at = format!("{} {} {}", e.account_id, e.currency, e.effective);
        if (e.account_id, e.currency, e.effective) != (a.account_id, a.currency, a.effective) {
            out.push(("row-set", format!("{at}: key mismatch with {a:?}")));
            continue;
        }
        let amounts = |s: &EffectiveBalanceSnapshot| {
            [
                s.settled.dr_balance,
                s.settled.cr_balance,
                s.pending.dr_balance,
                s.pending.cr_balance,
                s.encumbrance.dr_balance,
                s.encumbrance.cr_balance,
            ]
        };
        if amounts(e) != amounts(a) {
            out.push((
                "amounts",
                format!("{at}: {:?} != {:?}", amounts(e), amounts(a)),
            ));
        }
        if e.version != a.version {
            out.push(("version", format!("{at}: {} != {}", e.version, a.version)));
        }
        if e.all_time_version != a.all_time_version {
            out.push((
                "all_time_version",
                format!("{at}: {} != {}", e.all_time_version, a.all_time_version),
            ));
        }
        if e.entry_id != a.entry_id {
            out.push((
                "entry_id",
                format!("{at}: {} != {}", e.entry_id, a.entry_id),
            ));
        }
        for (layer, el, al) in [
            ("settled", &e.settled, &a.settled),
            ("pending", &e.pending, &a.pending),
            ("encumbrance", &e.encumbrance, &a.encumbrance),
        ] {
            if el.entry_id != al.entry_id {
                out.push((
                    "layer_entry_id",
                    format!("{at} {layer}: {} != {}", el.entry_id, al.entry_id),
                ));
            }
        }
    }
    out
}

async fn run_case(seed: u64, pool: &sqlx::PgPool) -> anyhow::Result<Vec<(&'static str, String)>> {
    let mut rng = StdRng::seed_from_u64(seed);
    let journal_id = JournalId::new();
    let accounts: Vec<AccountId> = (0..3).map(|_| AccountId::new()).collect();
    let base_date = NaiveDate::from_ymd_opt(2024, 1, 10).unwrap();
    let base_time = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
    let history = random_history(&mut rng, journal_id, &accounts, base_date, base_time);

    let repo = EffectiveBalanceRepo::new(pool);
    let mut model: HashMap<Pair, Vec<EffectiveBalanceSnapshot>> = HashMap::new();
    let mut mismatches = Vec::new();
    for (batch_no, batch) in history.iter().enumerate() {
        apply_batch_with_oracle(&mut model, journal_id, batch);
        apply_batch_with_sql(&repo, pool, journal_id, batch).await?;
        let actual = stored_rows(&repo, journal_id).await?;
        for (what, msg) in diff_rows(&last_row_per_date(&model), &actual) {
            mismatches.push((what, format!("seed {seed} batch {batch_no}: {msg}")));
        }
    }
    Ok(mismatches)
}

async fn pool() -> anyhow::Result<sqlx::PgPool> {
    let pg_con = std::env::var("PG_CON")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&pg_con)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// The SQL rule must reproduce the old fold in everything a reader sees:
/// amounts, versions, `all_time_version`, and the entry ids.
#[tokio::test]
async fn sql_delta_rule_matches_old_fold() -> anyhow::Result<()> {
    let pool = pool().await?;
    let mut all = Vec::new();
    for seed in 0..300 {
        all.extend(run_case(seed, &pool).await?);
    }
    let mut by_kind: HashMap<&str, usize> = HashMap::new();
    for (what, _) in &all {
        *by_kind.entry(*what).or_default() += 1;
    }
    assert!(
        all.is_empty(),
        "SQL rule diverges from the old fold: {by_kind:?}\nfirst few:\n{}",
        all.iter()
            .take(8)
            .map(|(w, m)| format!("[{w}] {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(())
}
