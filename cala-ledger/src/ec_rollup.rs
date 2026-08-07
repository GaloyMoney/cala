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
//! - **Single writer.** Registered via `register_event_handler`
//!   (`spawn_unique` underneath), so exactly one instance runs
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
        EventCtx, EventSubscription, FlushOp, Handled, OutboxEventHandler, OutboxEventJobConfig,
        PersistentOutboxEvent,
    },
    EventSequence, MailboxTables as _,
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
/// Must be called **before** [`Jobs::start_poll`] (`add_initializer`
/// panics once polling has started). Idempotent via `spawn_unique`.
pub(crate) async fn register_ec_balance_rollup(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    balances: &Balances,
    entries: &Entries,
) -> Result<(), crate::ledger::error::LedgerError> {
    outbox
        .register_event_handler(
            jobs,
            OutboxEventJobConfig::new(EC_BALANCE_ROLLUP_JOB)
                .with_max_batch_size(MAX_EVENTS_PER_BATCH),
            EcBalanceRollupHandler {
                balances: balances.clone(),
                entries: entries.clone(),
            },
        )
        .await
        .map_err(crate::ledger::error::LedgerError::EcRollupRegistration)
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

/// A point-in-time snapshot of the streaming EC-balance rollup's position:
/// the committed checkpoint (`applied`) against the persistent outbox
/// frontier (`frontier`). Every outbox event with sequence ≤ `applied` is
/// folded into EC balances (settled and effective) and committed.
///
/// [`lag`](Self::lag) doubles as the stream-lag SLO metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EcRollupStatus {
    /// The rollup job's committed checkpoint.
    pub applied: EventSequence,
    /// The highest outbox sequence assigned at snapshot time.
    pub frontier: EventSequence,
}

impl EcRollupStatus {
    /// Number of outbox sequence positions the rollup has yet to consume.
    ///
    /// Counts *all* outbox events — including the `BalanceUpdated` events
    /// the rollup itself publishes when it applies a batch, which it later
    /// crosses as skips (checkpointed lazily, bounded by the obix
    /// checkpoint interval). A healthy, fully-applied stream can therefore
    /// transiently report a small nonzero lag; alert on lag that is large
    /// or not shrinking, not on `!= 0`.
    pub fn lag(&self) -> u64 {
        u64::from(self.frontier).saturating_sub(u64::from(self.applied))
    }

    pub fn is_caught_up(&self) -> bool {
        self.applied >= self.frontier
    }
}

/// The rollup job's persisted execution state, written by the obix
/// event-handler runner (`{"sequence": <i64>}`). The runner commits the
/// checkpoint atomically with each applied batch, and over skip-only
/// stretches persists it lazily (bounded by the obix checkpoint interval)
/// — so the persisted value only ever *trails* the applied state, never
/// leads it.
#[derive(serde::Deserialize)]
struct RollupExecutionState {
    sequence: EventSequence,
}

/// Read the rollup job's committed checkpoint from the jobs table.
///
/// Deliberately a database read, not in-process state: the rollup is
/// `spawn_unique` — cluster-wide it runs on one node, while the caller
/// (an EOD orchestrator, a metrics poller) may be on another. A missing
/// row or a not-yet-persisted state both read as [`EventSequence::BEGIN`]:
/// a rollup that is not registered or has never checkpointed reports its
/// honest position, and [`await_caught_up`] surfaces it as a timeout
/// rather than a hang.
async fn applied_checkpoint(pool: &sqlx::PgPool) -> Result<EventSequence, LedgerError> {
    // Bind the `const` before borrowing: `as_str` on the constant itself
    // would borrow from a temporary that does not outlive the query.
    let job_type = EC_BALANCE_ROLLUP_JOB;
    let row = sqlx::query!(
        r#"
        SELECT execution_state_json
        FROM job_executions
        WHERE job_type = $1
        "#,
        job_type.as_str()
    )
    .fetch_optional(pool)
    .await?;
    Ok(row
        .and_then(|row| row.execution_state_json)
        .map(serde_json::from_value::<RollupExecutionState>)
        .transpose()
        .map_err(LedgerError::EcRollupStateDecode)?
        .map(|state| state.sequence)
        .unwrap_or(EventSequence::BEGIN))
}

/// Snapshot the outbox frontier from the persistent outbox's sequence
/// *generator* (not `MAX(sequence)` over the table): it covers sequences
/// already assigned to still-uncommitted (or aborted) postings, which is
/// what makes the close-books fence airtight — and it is invariant under
/// partition rotation or archival. Aborted sequences cannot wedge the
/// wait: obix gap-fill resolves them to placeholder deliveries the
/// checkpoint advances over.
async fn outbox_frontier(pool: &sqlx::PgPool) -> Result<EventSequence, LedgerError> {
    Ok(CalaMailboxTables::highest_known_persistent_sequence(pool).await?)
}

pub(crate) async fn rollup_status(pool: &sqlx::PgPool) -> Result<EcRollupStatus, LedgerError> {
    // Frontier first: a checkpoint read racing past a stale frontier can
    // only over-report progress relative to the snapshot, never lag that
    // is not there.
    let frontier = outbox_frontier(pool).await?;
    let applied = applied_checkpoint(pool).await?;
    Ok(EcRollupStatus { applied, frontier })
}

const INITIAL_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const MAX_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Await `applied ≥ frontier(call time)` by polling the persisted
/// checkpoint with backoff. See `CalaLedger::await_ec_caught_up` for the
/// semantics.
pub(crate) async fn await_caught_up(
    pool: &sqlx::PgPool,
    timeout: std::time::Duration,
) -> Result<(), LedgerError> {
    let started = tokio::time::Instant::now();
    let deadline = started + timeout;
    let frontier = outbox_frontier(pool).await?;
    let mut interval = INITIAL_POLL_INTERVAL;
    loop {
        let applied = applied_checkpoint(pool).await?;
        if applied >= frontier {
            return Ok(());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(LedgerError::EcCaughtUpTimeout {
                applied,
                frontier,
                waited: started.elapsed(),
            });
        }
        tokio::time::sleep(interval.min(deadline - now)).await;
        interval = (interval * 2).min(MAX_POLL_INTERVAL);
    }
}
