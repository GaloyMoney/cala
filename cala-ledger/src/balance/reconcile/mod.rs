//! # Entry-sourced verify/repair for eventually-consistent balances
//!
//! The streaming rollup ([`crate::ec_rollup`]) is delta-based and not
//! idempotent: it folds each committed transaction's deltas exactly once,
//! advancing its checkpoint atomically with every write. If an EC balance
//! ever drifts (applier bug, operator error, partial restore), nothing in
//! the steady-state machinery can recompute it. This module provides the
//! deterministic, bounded reconciliation path: [`Balances::verify_ec`]
//! computes the *expected* EC balances from the ledger's entries and
//! reports drift; [`Balances::repair_ec`] additionally appends corrective
//! snapshots (history stays append-only) and rebuilds the
//! cumulative-effective series for drifted pairs.
//!
//! ## The consistency anchor
//!
//! A naive "Σ entries as of now" rebuild double-counts: entries the
//! applier has not yet folded would be included in the rebuilt balance
//! *and* folded again later. And outbox sequence order is not commit
//! order, so "all committed entries right now" corresponds to no stream
//! position. The one clean consistency point is the rollup job's
//! committed cursor `C`: obix delivery is gapless, so every transaction
//! event with `seq ≤ C` is committed *and* already folded, and everything
//! above `C` will be folded later, exactly once. The expected state is
//! therefore the fold of entries belonging to transactions whose
//! `TransactionCreated` outbox event has `seq ≤ C` — computed while `C`
//! cannot move for the targets.
//!
//! Freezing `C` per target is free: the applier's flush takes SHARED
//! EC-class advisory locks on every account it writes
//! (`find_ec_balances_for_update`) in the same transaction that advances
//! the checkpoint. The reconciler takes the EXCLUSIVE counterpart on its
//! targets, so an in-flight applier batch touching them blocks *before*
//! writing or advancing `C`; its batch covers `(C, C']`, disjoint from the
//! reconciler's `≤ C` fold, and applies on top of the corrected state
//! after the reconciler commits.
//!
//! ## Why current membership is historical routing
//!
//! The fold routes each leaf entry to its EC ancestor sets using the
//! *current* membership graph (the downward inverse of the applier's
//! upward walk). That is sound because the membership guard
//! (`member_has_balance_history_in_op`) freezes every node on an applied
//! entry's ancestor path: the leaf has entries, and every ancestor set —
//! synchronous (inline history) or EC (history materialized at apply
//! time) — has balance history, so no edge on the path can be attached or
//! cut. The only members that can still move are those with zero applied
//! activity, which contribute nothing to the fold either way.
//!
//! ## Ground truth and retention
//!
//! Ground truth is `cala_entries` + each entry's `initialized` event —
//! never member balance history. The `tx event seq ≤ C` mapping is read
//! from the persistent outbox, which obix range-partitions but never
//! prunes; if outbox pruning/archival ever lands, this scan needs an
//! alternative "entries as of `C`" source.

mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use tracing::instrument;

use std::collections::{BTreeMap, HashMap};

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot, EffectiveBalanceSnapshot},
    entry::EntryValues,
    primitives::{AccountId, Currency, EntryId, JournalId},
};

use crate::{ec_rollup::EC_BALANCE_ROLLUP_JOB_NAME, journal::Journal};

use super::{
    error::BalanceError,
    snapshot::{Snapshots, UNASSIGNED_ENTRY_ID},
    Balances,
};

/// Targets reconciled per atomic operation: bounds the advisory locks
/// held, the fold's working set and the size of any repair commit.
const TARGETS_PER_RECONCILE_OP: usize = 100;

/// Rows per keyset-paginated contributing-entry scan — bounds the
/// reconciler's memory per round trip (the running fold itself holds one
/// snapshot per involved `(account, currency)` pair, never a history).
const ENTRY_SCAN_CHUNK: i64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReconcileMode {
    Verify,
    Repair,
}

/// Drift of one balance layer/direction pair: `expected − found`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerDrift {
    pub dr_delta: Decimal,
    pub cr_delta: Decimal,
}

impl LayerDrift {
    fn between(expected: Option<&BalanceAmount>, found: Option<&BalanceAmount>) -> Self {
        let zero = Decimal::ZERO;
        Self {
            dr_delta: expected.map(|a| a.dr_balance).unwrap_or(zero)
                - found.map(|a| a.dr_balance).unwrap_or(zero),
            cr_delta: expected.map(|a| a.cr_balance).unwrap_or(zero)
                - found.map(|a| a.cr_balance).unwrap_or(zero),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.dr_delta == Decimal::ZERO && self.cr_delta == Decimal::ZERO
    }
}

/// Verification result for one `(account, currency)` pair. Any nonzero
/// drift is evidence of an applier bug (or external interference) — the
/// report is the evidence; keep it.
#[derive(Debug, Clone)]
pub struct EcDriftReport {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
    /// `expected − found` per layer.
    pub settled: LayerDrift,
    pub pending: LayerDrift,
    pub encumbrance: LayerDrift,
    /// Version the fold reproduces (one bump per folded entry) — what a
    /// single correct applier run would have left behind. Evidence, not a
    /// drift trigger: a previously repaired pair legitimately carries a
    /// higher found version (the appended corrective snapshot).
    pub expected_version: u32,
    /// The current snapshot's version; `None` when no row exists.
    pub found_version: Option<u32>,
    /// The latest cumulative-effective snapshot's totals disagree with
    /// the expected fold (only checked when the journal maintains
    /// effective balances).
    pub effective_drift: bool,
    /// Set by [`Balances::repair_ec`] when a corrective write was issued
    /// for this pair.
    pub repaired: bool,
}

impl EcDriftReport {
    /// Drift in the balance values themselves (any layer/direction).
    /// Versions are deliberately not compared: they are chain
    /// bookkeeping, and a corrective append leaves the found version
    /// above the fold's count by design.
    pub fn balance_drift(&self) -> bool {
        !self.settled.is_zero() || !self.pending.is_zero() || !self.encumbrance.is_zero()
    }

    pub fn is_drifted(&self) -> bool {
        self.balance_drift() || self.effective_drift
    }
}

impl Balances {
    /// Verify the eventually-consistent balances of `account_ids`
    /// (EC set-backing accounts and/or EC plain accounts) against the
    /// expected fold of their contributing entries as of the streaming
    /// rollup's committed cursor. Read-only; returns one report per
    /// involved `(account, currency)` pair.
    ///
    /// Takes the same exclusive locks as [`repair_ec`](Self::repair_ec)
    /// so the report is race-free against the applier — cheap, and this
    /// is a rare operational tool.
    #[instrument(
        name = "cala_ledger.balance.verify_ec",
        skip(self, account_ids),
        fields(journal_id = %journal_id, targets = account_ids.len()),
        err(level = "warn")
    )]
    pub async fn verify_ec(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<Vec<EcDriftReport>, BalanceError> {
        self.reconcile_ec(journal_id, account_ids, ReconcileMode::Verify)
            .await
    }

    /// [`verify_ec`](Self::verify_ec) plus repair: for every drifted
    /// pair, append a corrective `BalanceSnapshot` (history stays
    /// append-only — no rows are rewritten or deleted) and, when the
    /// journal maintains effective balances, delete-and-reinsert the
    /// cumulative-effective series (a projection, not the audit log).
    ///
    /// Idempotent at the state level: once repaired, a second run finds
    /// zero drift and writes nothing. Each chunk of targets commits
    /// independently, so a run that fails midway leaves earlier chunks
    /// repaired and is safe to re-run.
    #[instrument(
        name = "cala_ledger.balance.repair_ec",
        skip(self, account_ids),
        fields(journal_id = %journal_id, targets = account_ids.len()),
        err(level = "warn")
    )]
    pub async fn repair_ec(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<Vec<EcDriftReport>, BalanceError> {
        self.reconcile_ec(journal_id, account_ids, ReconcileMode::Repair)
            .await
    }

    async fn reconcile_ec(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        mode: ReconcileMode,
    ) -> Result<Vec<EcDriftReport>, BalanceError> {
        let journal = self.journals.find(journal_id).await?;
        if mode == ReconcileMode::Repair && journal.is_locked() {
            return Err(BalanceError::JournalLocked(journal.id));
        }

        let mut targets: Vec<AccountId> = account_ids.to_vec();
        targets.sort();
        targets.dedup();

        let mut reports = Vec::new();
        for chunk in targets.chunks(TARGETS_PER_RECONCILE_OP) {
            reports.extend(self.reconcile_chunk(&journal, chunk, mode).await?);
        }
        Ok(reports)
    }

    /// Reconcile one bounded chunk of targets in its own atomic
    /// operation: lock → read cursor → fold → compare → (repair) →
    /// commit.
    async fn reconcile_chunk(
        &self,
        journal: &Journal,
        targets: &[AccountId],
        mode: ReconcileMode,
    ) -> Result<Vec<EcDriftReport>, BalanceError> {
        let journal_id = journal.id;
        let mut op = es_entity::DbOp::init(&self.pool).await?;

        // 1. Exclusive locks on the (sorted, deduped) targets — the
        //    serialization point against the streaming applier.
        repo::lock_targets_exclusive(&mut op, targets).await?;

        // 2. Only *after* the locks: the cursor is now frozen for any
        //    applier batch touching the targets.
        let set_targets = repo::load_set_backed_targets(&mut op, targets).await?;
        let cursor = repo::ec_rollup_cursor(&mut op, EC_BALANCE_ROLLUP_JOB_NAME).await?;

        // 3. Routing map: leaf account → the targets its entries fold
        //    into. Set-backed targets expand downward through the
        //    membership graph; EC plain-account targets route to
        //    themselves.
        let mut routing: HashMap<AccountId, Vec<AccountId>> = HashMap::new();
        for (target, leaf) in repo::expand_set_targets(&mut op, &set_targets).await? {
            let routes = routing.entry(leaf).or_default();
            if !routes.contains(&target) {
                routes.push(target);
            }
        }
        for target in targets {
            if !set_targets.contains(target) {
                let routes = routing.entry(*target).or_default();
                if !routes.contains(target) {
                    routes.push(*target);
                }
            }
        }
        let leaf_ids: Vec<AccountId> = routing.keys().copied().collect();

        // 4. Streaming fold of the contributing entries ≤ cursor, in
        //    applier order (tx event seq, then entry sequence), chunk by
        //    chunk — the running state is one snapshot per involved
        //    pair, never a materialized history.
        let mut expected: HashMap<(AccountId, Currency), BalanceSnapshot> = HashMap::new();
        let mut effective_series = (mode == ReconcileMode::Repair
            && journal.insert_effective_balances())
        .then(EffectiveSeriesBuilder::default);

        if cursor > 0 && !leaf_ids.is_empty() {
            let mut after: Option<(i64, i32)> = None;
            loop {
                let batch = repo::contributing_entries_chunk(
                    &mut op,
                    journal_id,
                    cursor,
                    &leaf_ids,
                    after,
                    ENTRY_SCAN_CHUNK,
                )
                .await?;
                let n = batch.len();
                for row in batch {
                    let entry: EntryValues = serde_json::from_value(row.entry_values)
                        .expect("Failed to deserialize entry values");
                    after = Some((row.tx_seq, row.entry_seq));
                    let Some(routes) = routing.get(&entry.account_id) else {
                        continue;
                    };
                    for target in routes {
                        let key = (*target, entry.currency);
                        let next = match expected.remove(&key) {
                            Some(prev) => {
                                Snapshots::update_snapshot(row.tx_created_at, prev, &entry)
                            }
                            None => Snapshots::new_snapshot(row.tx_created_at, *target, &entry),
                        };
                        expected.insert(key, next);
                        if let Some(series) = effective_series.as_mut() {
                            series.apply(*target, row.effective, row.tx_created_at, &entry);
                        }
                    }
                }
                if n < ENTRY_SCAN_CHUNK as usize {
                    break;
                }
            }
        }

        // 5. Found state (still under the locks).
        let current = repo::current_balances(&mut op, journal_id, targets).await?;
        let history_max = repo::max_history_versions(&mut op, journal_id, targets).await?;
        let effective_current = if journal.insert_effective_balances() {
            repo::latest_effective_balances(&mut op, journal_id, targets).await?
        } else {
            HashMap::new()
        };

        // 6. Compare per pair; collect repairs.
        let mut pairs: Vec<(AccountId, Currency)> = expected
            .keys()
            .chain(current.keys())
            .chain(effective_current.keys())
            .copied()
            .collect();
        pairs.sort_by(|a, b| (a.0, a.1.code()).cmp(&(b.0, b.1.code())));
        pairs.dedup();

        let mut reports = Vec::with_capacity(pairs.len());
        let mut correctives: Vec<BalanceSnapshot> = Vec::new();
        let mut effective_rebuild: Vec<(AccountId, Currency)> = Vec::new();

        for pair in pairs {
            let exp = expected.get(&pair);
            let cur = current.get(&pair);
            let effective_drift = journal.insert_effective_balances()
                && match (exp, effective_current.get(&pair)) {
                    (None, None) => false,
                    (Some(e), Some(c)) => !amounts_equal(e, c),
                    (Some(_), None) | (None, Some(_)) => true,
                };

            let mut report = EcDriftReport {
                journal_id,
                account_id: pair.0,
                currency: pair.1,
                settled: LayerDrift::between(exp.map(|s| &s.settled), cur.map(|s| &s.settled)),
                pending: LayerDrift::between(exp.map(|s| &s.pending), cur.map(|s| &s.pending)),
                encumbrance: LayerDrift::between(
                    exp.map(|s| &s.encumbrance),
                    cur.map(|s| &s.encumbrance),
                ),
                expected_version: exp.map(|s| s.version).unwrap_or(0),
                found_version: cur.map(|s| s.version),
                effective_drift,
                repaired: false,
            };

            if mode == ReconcileMode::Repair {
                if report.balance_drift() {
                    correctives.push(corrective_snapshot(
                        journal_id,
                        pair,
                        exp,
                        report.found_version,
                        history_max.get(&pair).copied().unwrap_or(0),
                    ));
                    report.repaired = true;
                }
                if report.effective_drift {
                    effective_rebuild.push(pair);
                    report.repaired = true;
                }
            }
            if report.is_drifted() {
                tracing::warn!(
                    report = ?report,
                    "eventually-consistent balance drift detected"
                );
            }
            reports.push(report);
        }

        // 7. Repair writes: corrective snapshots append through the
        //    normal insert path; the effective series is rebuilt
        //    wholesale for drifted pairs (one row per effective date).
        if !correctives.is_empty() {
            self.repo
                .insert_new_snapshots(&mut op, journal_id, correctives)
                .await?;
        }
        if !effective_rebuild.is_empty() {
            let series = effective_series
                .as_ref()
                .expect("effective series is built in repair mode");
            let mut snapshots = Vec::new();
            for pair in &effective_rebuild {
                snapshots.extend(series.series_for(*pair, journal_id));
            }
            self.effective
                .rebuild_ec_series_in_op(&mut op, journal_id, &effective_rebuild, snapshots)
                .await?;
        }

        op.commit().await?;
        Ok(reports)
    }
}

fn amounts_equal(a: &BalanceSnapshot, b: &BalanceSnapshot) -> bool {
    a.settled.dr_balance == b.settled.dr_balance
        && a.settled.cr_balance == b.settled.cr_balance
        && a.pending.dr_balance == b.pending.dr_balance
        && a.pending.cr_balance == b.pending.cr_balance
        && a.encumbrance.dr_balance == b.encumbrance.dr_balance
        && a.encumbrance.cr_balance == b.encumbrance.cr_balance
}

/// The snapshot appended to correct a drifted pair.
///
/// Values are the expected fold's; the version clears both the found
/// `latest_version` and the highest history row (the history version
/// column is unique per balance). A pair whose chain is entirely absent
/// keeps the fold's own version — the single appended row carries the
/// applier-identical end state (intermediate versions are not
/// re-materialized). A phantom pair (no expected entries but a balance
/// row exists) is zeroed out one version above everything found.
fn corrective_snapshot(
    journal_id: JournalId,
    (account_id, currency): (AccountId, Currency),
    expected: Option<&BalanceSnapshot>,
    found_version: Option<u32>,
    max_history_version: u32,
) -> BalanceSnapshot {
    let floor = found_version.unwrap_or(0).max(max_history_version);
    match expected {
        Some(snapshot) => {
            let mut corrective = snapshot.clone();
            if floor > 0 {
                corrective.version = floor.max(corrective.version) + 1;
            }
            corrective
        }
        None => {
            let mut corrective = zero_snapshot(journal_id, account_id, currency, Utc::now());
            corrective.version = floor + 1;
            corrective
        }
    }
}

fn zero_snapshot(
    journal_id: JournalId,
    account_id: AccountId,
    currency: Currency,
    time: DateTime<Utc>,
) -> BalanceSnapshot {
    let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);
    let zero_amount = BalanceAmount {
        dr_balance: Decimal::ZERO,
        cr_balance: Decimal::ZERO,
        entry_id,
        modified_at: time,
    };
    BalanceSnapshot {
        journal_id,
        account_id,
        currency,
        entry_id,
        settled: zero_amount.clone(),
        pending: zero_amount.clone(),
        encumbrance: zero_amount,
        version: 0,
        modified_at: time,
        created_at: time,
    }
}

/// Accumulates per-day *delta* folds during the entry scan and produces
/// the rebuilt cumulative-effective series: one row per effective date,
/// `version` = entries folded that day, `all_time_version` = entries
/// folded up to and including that day. Memory is bounded by
/// `pairs × distinct effective dates` — never per-entry.
///
/// Intra-day per-entry rows are deliberately not reproduced: the
/// applier's incremental history depends on arrival-order interleaving
/// (back-dated rewrites bump `all_time_version` per rewritten row),
/// which is not reconstructible from the ledger. The rebuilt series
/// preserves every read-path semantic (`effective ≤ date` lookups and
/// `all_time_version` diffs as entry counts in range).
#[derive(Default)]
struct EffectiveSeriesBuilder {
    days: HashMap<(AccountId, Currency), BTreeMap<NaiveDate, BalanceSnapshot>>,
}

impl EffectiveSeriesBuilder {
    fn apply(
        &mut self,
        target: AccountId,
        effective: NaiveDate,
        time: DateTime<Utc>,
        entry: &EntryValues,
    ) {
        let days = self.days.entry((target, entry.currency)).or_default();
        let next = match days.remove(&effective) {
            Some(prev) => Snapshots::update_snapshot(time, prev, entry),
            None => Snapshots::new_snapshot(time, target, entry),
        };
        days.insert(effective, next);
    }

    fn series_for(
        &self,
        pair: (AccountId, Currency),
        journal_id: JournalId,
    ) -> Vec<EffectiveBalanceSnapshot> {
        let Some(days) = self.days.get(&pair) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(days.len());
        let mut cumulative: Option<BalanceSnapshot> = None;
        let mut all_time_version = 0u32;
        for (date, delta) in days {
            let snapshot = match &cumulative {
                None => delta.clone(),
                Some(prev) => merge_day(prev, delta),
            };
            all_time_version += delta.version;
            out.push(EffectiveBalanceSnapshot {
                journal_id,
                account_id: pair.0,
                currency: pair.1,
                effective: *date,
                version: delta.version,
                all_time_version,
                created_at: snapshot.created_at,
                modified_at: snapshot.modified_at,
                entry_id: snapshot.entry_id,
                settled: snapshot.settled.clone(),
                pending: snapshot.pending.clone(),
                encumbrance: snapshot.encumbrance.clone(),
            });
            cumulative = Some(snapshot);
        }
        out
    }
}

/// Merge one day's delta fold onto the previous cumulative snapshot:
/// touched layers add their deltas (keeping the day's last entry id);
/// untouched layers carry the previous cumulative amount forward.
fn merge_day(prev: &BalanceSnapshot, delta: &BalanceSnapshot) -> BalanceSnapshot {
    let mut snapshot = delta.clone();
    snapshot.created_at = prev.created_at;
    merge_amount(&mut snapshot.settled, &prev.settled);
    merge_amount(&mut snapshot.pending, &prev.pending);
    merge_amount(&mut snapshot.encumbrance, &prev.encumbrance);
    snapshot
}

fn merge_amount(day: &mut BalanceAmount, prev: &BalanceAmount) {
    if day.entry_id == EntryId::from(UNASSIGNED_ENTRY_ID) {
        *day = prev.clone();
    } else {
        day.dr_balance += prev.dr_balance;
        day.cr_balance += prev.cr_balance;
    }
}
