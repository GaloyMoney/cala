//! Streaming rollup of eventually-consistent (EC) account-set balances.
//!
//! A single long-lived outbox event-handler job consumes the obix outbox
//! in `sequence` order and rolls each committed transaction's leaf-entry
//! deltas up into its ancestor **EC** account sets — incrementally and
//! bounded. This replaces the periodic pull/batch
//! `recalculate_balances_deep` as the steady-state mechanism (which could
//! OOM a Postgres backend by replaying a whole set's history in one
//! transaction). Work here is proportional to *new* activity and every
//! commit is size-bounded.
//!
//! ## Shape
//!
//! Built on obix's managed [`OutboxEventHandler`] batching runner:
//! `TransactionCreated` and `EntryCreated` events are collected into the
//! pending batch (pure memory writes — no transaction per event),
//! everything else is skipped. When the batch lands the runner calls
//! [`flush`](OutboxEventHandler::flush) once, **inside the transaction
//! that commits the checkpoint** — the rollup writes and the stream
//! position land atomically. Entries are applied straight from the
//! stream when a transaction's whole event group landed in the batch
//! (verified against `TransactionValues::entry_ids`), with a DB read
//! through the flush op as the fallback.
//!
//! ## Correctness
//!
//! - **Exactly-once DB effect.** The applier *adds* deltas (it is not
//!   idempotent), so it must never re-run for an already-applied event.
//!   The runner guarantees this: flushed items and the checkpoint commit
//!   in one transaction, so a mid-batch crash rolls back both and replay
//!   re-collects only unapplied events.
//! - **Single writer.** Registered via `register_event_handler` (a
//!   *resident* job underneath), so exactly one instance runs
//!   cluster-wide — no streaming-vs-streaming contention.
//! - **Sole EC-set writer.** There is no separate pull/batch recalc to
//!   compose with — this job is the only maintainer of EC-set balances.
//!   The applier takes the shared EC-set advisory lock on the sets it
//!   writes (matching the poster lock discipline), but being the only
//!   EC-set writer it needs no coordination with posters (which never
//!   write EC-set balances).
//! - **No membership trigger.** A member can only join/leave an EC set
//!   while it has no balance history (`MemberHasBalanceHistory`), so
//!   membership carries no balance to seed/unfold — the live closure alone
//!   routes future entries.

use chrono::{DateTime, NaiveDate, Utc};

use std::collections::{HashMap, HashSet};

use job::{JobType, Jobs};
use obix::{
    out::{
        EventCtx, EventSubscription, FlushOp, Handled, HandlerStreamStatus, OutboxEventHandler,
        OutboxEventJobConfig, PersistentOutboxEvent, RegisteredEventHandler,
    },
    EventSequence,
};

use cala_types::entry::EntryValues;

use crate::{
    balance::{Balances, EcRollupTxn},
    entry::{Entries, Entry},
    ledger::error::LedgerError,
    outbox::{CalaMailboxTables, ObixOutbox, OutboxEventPayload},
    primitives::{EntryId, JournalId, TransactionId},
};

const EC_BALANCE_ROLLUP_JOB: JobType = JobType::new("cala.ec_balance_rollup");

/// Maximum number of collected events (transactions + their entries)
/// folded into a single commit. Bounds per-transaction memory/WAL/lock
/// hold-time. The per-statement insert is additionally sub-chunked inside
/// `insert_new_snapshots`.
const MAX_EVENTS_PER_BATCH: usize = 1_000;

/// Register the streaming EC-balance rollup and spawn its single instance.
///
/// Must be called **before** [`Jobs::start_poll`]
/// (`add_resident_initializer` panics once polling has started).
/// Idempotent via the resident-job spawn. The returned handle is the
/// ledger's observation point on the rollup.
pub(crate) async fn register_ec_balance_rollup(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    balances: &Balances,
    entries: &Entries,
) -> Result<RegisteredEventHandler<OutboxEventPayload, CalaMailboxTables>, LedgerError> {
    Ok(outbox
        .register_event_handler(
            jobs,
            OutboxEventJobConfig::new(EC_BALANCE_ROLLUP_JOB)
                .with_max_batch_size(MAX_EVENTS_PER_BATCH),
            EcBalanceRollupHandler {
                balances: balances.clone(),
                entries: entries.clone(),
            },
        )
        .await?)
}

/// A transaction pulled from a `TransactionCreated` event, carrying just
/// what the rollup needs. `entry_ids` is the complete expected entry set,
/// which is what makes stream-collected entries verifiable (see
/// [`EcRollupBatch`]).
struct PendingTx {
    id: TransactionId,
    journal_id: JournalId,
    effective: NaiveDate,
    created_at: DateTime<Utc>,
    entry_ids: Vec<EntryId>,
}

/// One batch landing's accumulator.
///
/// Entries are collected best-effort from the `EntryCreated` events that
/// share the landing with their transaction. A transaction's event group
/// is *not* guaranteed to land whole: the runner counts events (not
/// groups) against `max_batch_size`, and concurrent postings interleave
/// sequences — so a group can straddle two landings. `PendingTx::entry_ids`
/// makes completeness decidable per transaction at flush time; incomplete
/// groups fall back to a DB read (the entries committed atomically with
/// the `TransactionCreated` event, so they are always visible). Straggler
/// entries whose transaction flushed in an earlier landing are simply
/// dropped — their data is durable in the ledger and was already applied
/// via that landing's fallback read.
#[derive(Default)]
struct EcRollupBatch {
    txns: Vec<PendingTx>,
    entries: HashMap<TransactionId, Vec<EntryValues>>,
}

impl EcRollupBatch {
    fn push_tx(&mut self, tx: PendingTx) {
        self.txns.push(tx);
    }

    fn push_entry(&mut self, entry: EntryValues) {
        self.entries
            .entry(entry.transaction_id)
            .or_default()
            .push(entry);
    }

    /// Entry ids that were *not* collected from the stream in this landing
    /// (their event group straddled a landing boundary) — the ones the
    /// flush must load from the DB.
    fn missing_entry_ids(&self) -> Vec<EntryId> {
        self.txns
            .iter()
            .flat_map(|tx| {
                let collected: HashSet<EntryId> = self
                    .entries
                    .get(&tx.id)
                    .map(|entries| entries.iter().map(|e| e.id).collect())
                    .unwrap_or_default();
                tx.entry_ids
                    .iter()
                    .copied()
                    .filter(move |id| !collected.contains(id))
            })
            .collect()
    }

    /// Assemble the applier's input in landing order: each transaction's
    /// stream-collected entries, topped up from the DB-`fetched` map where
    /// the group straddled a landing boundary, sorted by entry sequence.
    fn into_rollup_txns(self, mut fetched: HashMap<EntryId, Entry>) -> Vec<EcRollupTxn> {
        let EcRollupBatch { txns, mut entries } = self;
        txns.into_iter()
            .map(|tx| {
                let mut entry_values = entries.remove(&tx.id).unwrap_or_default();
                if entry_values.len() != tx.entry_ids.len() {
                    entry_values.extend(
                        tx.entry_ids
                            .iter()
                            .filter_map(|id| fetched.remove(id))
                            .map(Entry::into_values),
                    );
                }
                entry_values.sort_by_key(|e| e.sequence);

                EcRollupTxn {
                    journal_id: tx.journal_id,
                    effective: tx.effective,
                    created_at: tx.created_at,
                    entries: entry_values,
                }
            })
            .collect()
    }
}

struct EcBalanceRollupHandler {
    balances: Balances,
    entries: Entries,
}

impl OutboxEventHandler<OutboxEventPayload> for EcBalanceRollupHandler {
    const SUBSCRIPTION: EventSubscription = EventSubscription::PersistentOnly;

    type Batch = EcRollupBatch;

    async fn handle_persistent<'inv>(
        &self,
        ctx: EventCtx<'inv, Self::Batch>,
        event: &PersistentOutboxEvent<OutboxEventPayload>,
    ) -> Result<Handled<'inv>, Box<dyn std::error::Error + Send + Sync>> {
        match &event.payload {
            Some(OutboxEventPayload::TransactionCreated { transaction }) => {
                let tx = PendingTx {
                    id: transaction.id,
                    journal_id: transaction.journal_id,
                    effective: transaction.effective,
                    created_at: transaction.created_at,
                    entry_ids: transaction.entry_ids.clone(),
                };
                Ok(ctx.collect_with(|batch| batch.push_tx(tx)))
            }
            Some(OutboxEventPayload::EntryCreated { entry }) => {
                let entry = entry.clone();
                Ok(ctx.collect_with(|batch| batch.push_entry(entry)))
            }
            _ => Ok(ctx.skip()),
        }
    }

    #[tracing::instrument(
        name = "cala_ledger.ec_rollup.flush",
        skip_all,
        fields(txns_count = batch.txns.len()),
        err(level = "warn")
    )]
    async fn flush(
        &self,
        op: &mut FlushOp<'_>,
        batch: Self::Batch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let missing_ids = batch.missing_entry_ids();
        let fetched = if missing_ids.is_empty() {
            HashMap::new()
        } else {
            self.entries.find_all_in_op(op, &missing_ids).await?
        };

        let rollup_txns = batch.into_rollup_txns(fetched);
        self.balances.apply_ec_rollup_in_op(op, rollup_txns).await?;
        Ok(())
    }
}

#[cfg(feature = "fuzz")]
mod __fuzz {
    //! Harness for the out-of-tree `ec_rollup_batch` fuzz target. Lives in
    //! this module so it can reach the private `EcRollupBatch`/`PendingTx`.
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct FuzzTx {
        id: TransactionId,
        journal_id: JournalId,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        entry_ids: Vec<EntryId>,
    }

    pub fn fuzz_batch(data: &[u8]) {
        let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
        if parts.len() < 2 {
            return;
        }
        let Ok(txs) = serde_json::from_slice::<Vec<FuzzTx>>(parts[0]) else {
            return;
        };
        let Ok(entries) = serde_json::from_slice::<Vec<EntryValues>>(parts[1]) else {
            return;
        };

        let mut batch = EcRollupBatch::default();
        for t in &txs {
            batch.push_tx(PendingTx {
                id: t.id,
                journal_id: t.journal_id,
                effective: t.effective,
                created_at: t.created_at,
                entry_ids: t.entry_ids.clone(),
            });
        }
        for e in &entries {
            batch.push_entry(e.clone());
        }

        let _missing = batch.missing_entry_ids();
        let _rollup = batch.into_rollup_txns(HashMap::<EntryId, Entry>::new());
    }
}

#[cfg(feature = "fuzz")]
pub use __fuzz::fuzz_batch;

/// A snapshot of the rollup's position. Every outbox event with sequence ≤
/// `applied` is folded into EC balances (settled and effective) and
/// committed.
///
/// `frontier` is pinned at construction. [`refresh`](Self::refresh) advances
/// `applied` against that same fence, so [`lag`](Self::lag) drains toward it
/// instead of chasing a frontier that new postings keep moving.
#[derive(Debug, Clone)]
pub struct EcRollupStatus {
    /// The rollup job's committed checkpoint.
    pub applied: EventSequence,
    /// The outbox frontier pinned when this snapshot was taken.
    pub frontier: EventSequence,
    handle: RegisteredEventHandler<OutboxEventPayload, CalaMailboxTables>,
}

impl EcRollupStatus {
    pub(crate) fn new(
        status: HandlerStreamStatus,
        handle: RegisteredEventHandler<OutboxEventPayload, CalaMailboxTables>,
    ) -> Self {
        Self {
            applied: status.checkpoint,
            frontier: status.frontier,
            handle,
        }
    }

    /// Re-read the committed checkpoint, keeping the pinned `frontier`, so
    /// repeated calls watch the lag drain toward the fence this snapshot
    /// captured.
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.ec_rollup_status.refresh",
        skip_all,
        fields(frontier = %self.frontier, applied, lag)
    )]
    pub async fn refresh(&mut self) -> Result<(), LedgerError> {
        self.applied = self.handle.load().await?.checkpoint();

        let span = tracing::Span::current();
        span.record("applied", u64::from(self.applied));
        span.record("lag", self.lag());

        Ok(())
    }

    /// Await the rollup applying everything up to this snapshot's pinned
    /// `frontier`.
    ///
    /// On `Ok(())` every posting that had been assigned an outbox sequence
    /// when the snapshot was taken — committed or still in flight — is folded
    /// into EC balances (settled and effective) and visible to subsequent
    /// reads.
    ///
    /// This is what makes `close_books(); ec_rollup_status().await?
    /// .await_completion(..)` free of straggler holes: sequences are assigned
    /// at entry insert, *before* velocity enforcement, so anything that saw
    /// the period as open sits at or below the pinned frontier, and gapless
    /// delivery means the wait covers each one. The checkpoint only *trails*
    /// the applied state, so the fence never returns early.
    ///
    /// The fence does not move: unlike re-reading status, the frontier stays
    /// where the snapshot pinned it, so a rollup publishing `BalanceUpdated`
    /// events as it drains cannot extend its own barrier.
    ///
    /// `timeout` is mandatory: a wedged rollup surfaces as
    /// [`LedgerError::EcCaughtUpTimeout`], never a silent hang.
    pub async fn await_completion(&self, timeout: std::time::Duration) -> Result<(), LedgerError> {
        self.handle.await_sequence(self.frontier, timeout).await?;
        Ok(())
    }

    /// Outbox positions the rollup has yet to consume — the stream-lag SLO
    /// metric. Counts the `BalanceUpdated` events the rollup publishes
    /// itself and later crosses as skips, so a healthy stream can report a
    /// small nonzero lag; alert on lag that is large or not shrinking.
    pub fn lag(&self) -> u64 {
        u64::from(self.frontier).saturating_sub(u64::from(self.applied))
    }

    pub fn is_caught_up(&self) -> bool {
        self.applied >= self.frontier
    }
}
