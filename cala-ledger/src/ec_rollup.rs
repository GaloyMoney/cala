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

use std::collections::HashMap;

use job::{JobType, Jobs};
use obix::out::{
    EventCtx, EventSubscription, FlushOp, Handled, OutboxEventHandler, OutboxEventJobConfig,
    PersistentOutboxEvent,
};

use cala_types::entry::EntryValues;

use crate::{
    balance::{Balances, EcRollupTxn},
    entry::Entries,
    outbox::{ObixOutbox, OutboxEventPayload},
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

struct EcBalanceRollupHandler {
    balances: Balances,
    entries: Entries,
}

impl OutboxEventHandler<OutboxEventPayload> for EcBalanceRollupHandler {
    // cala only publishes persistent events; never subscribe the
    // ephemeral stream.
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
                Ok(ctx.collect_with(|batch| batch.txns.push(tx)))
            }
            Some(OutboxEventPayload::EntryCreated { entry }) => {
                let entry = entry.clone();
                Ok(ctx.collect_with(|batch| {
                    batch
                        .entries
                        .entry(entry.transaction_id)
                        .or_default()
                        .push(entry)
                }))
            }
            _ => Ok(ctx.skip()),
        }
    }

    // Why collect/flush rather than `consume_in_batch`/`defer`: the
    // transaction/checkpoint economics are the same (one commit per
    // landing either way), but with collect the per-event path is a pure
    // memory write — no batch transaction is open (and no EC advisory
    // locks are held) while the backlog drains through the handler — and
    // flush sees the whole batch at once, which is what enables sourcing
    // entries from the stream itself and handing the applier the entire
    // batch (one lock/read/insert pass per journal instead of one per
    // transaction). `defer` is structurally one-event-at-a-time and can
    // do neither.
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
        let EcRollupBatch { txns, mut entries } = batch;

        // One read for every transaction whose event group straddled a
        // landing boundary (the entries committed atomically with the
        // `TransactionCreated` event, so they are always visible).
        let missing_ids: Vec<EntryId> = txns
            .iter()
            .filter(|tx| entries.get(&tx.id).map_or(0, Vec::len) != tx.entry_ids.len())
            .flat_map(|tx| tx.entry_ids.iter().copied())
            .collect();
        let mut fetched = if missing_ids.is_empty() {
            HashMap::new()
        } else {
            self.entries.find_all_in_op(op, &missing_ids).await?
        };

        let mut rollup_txns = Vec::with_capacity(txns.len());
        for tx in txns {
            let collected = entries.remove(&tx.id).unwrap_or_default();
            let mut entry_values = if collected.len() == tx.entry_ids.len() {
                // The whole event group landed in this batch — no DB read.
                collected
            } else {
                tx.entry_ids
                    .iter()
                    .filter_map(|id| fetched.remove(id))
                    .map(|entry| entry.into_values())
                    .collect()
            };
            entry_values.sort_by_key(|e| e.sequence);

            rollup_txns.push(EcRollupTxn {
                journal_id: tx.journal_id,
                effective: tx.effective,
                created_at: tx.created_at,
                entries: entry_values,
            });
        }

        self.balances.apply_ec_rollup_in_op(op, rollup_txns).await?;
        Ok(())
    }
}
