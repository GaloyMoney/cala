//! Streaming maintenance of calculation effective balance series.
//!
//! A single long-lived outbox subscriber consumes the obix outbox in
//! `sequence` order and folds each committed transaction's entries into the
//! series of every calculation that covers the entry's account or one of
//! its ancestor sets. It is the sole writer of
//! `cala_calculation_effective_balances` and has its own checkpoint, so the
//! posting path and the EC rollup are unaffected by calculations.
//!
//! ## Shape
//!
//! Built on obix's managed [`SingletonSubscriber`] batching runner, like
//! [`crate::ec_rollup`]: `TransactionCreated` and `EntryCreated` events are
//! collected into the pending batch, everything else is skipped. When the
//! batch lands the runner calls [`flush`](SingletonSubscriber::flush) once,
//! **inside the transaction that commits the checkpoint**, so the series
//! writes and the stream position land atomically. Entries are applied
//! straight from the stream when a transaction's whole event group landed
//! in the batch, with a DB read through the flush op as the fallback.
//!
//! Every flush opens by taking the SHARED side of the calculation handoff
//! lock, before it reads anything; a backfill's handoff takes the
//! EXCLUSIVE side, so it waits for in-flight flushes and later flushes see
//! the unit `Complete`. A ledger without calculations costs that lock and
//! one `EXISTS` probe per landing.
//!
//! ## Coverage
//!
//! A calculation applies to a transaction whose outbox sequence is greater
//! than the calculation's `activation_sequence` (Global) or the target's
//! attachment `activation_sequence` (Attached). A CEL evaluation error for
//! one entry is logged and the entry treated as not matching that
//! calculation; it never fails the flush.

use crate::error::CalaFault;
use es_entity::errlanes::{lanes, Fail, ResultExt};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use job::{JobType, Jobs};
use obix::{
    out::{
        EventCtx, EventDelivery, FlushOp, Handled, InsertOrder, OutboxEventJobConfig,
        PersistentOutboxEvent, SingletonSubscriber, Subscription,
    },
    EventSequence,
};

use cala_types::{entry::EntryValues, transaction::TransactionValues};

use crate::{
    calculation::{CalculationRollupTxn, Calculations, RollupScope},
    entry::{Entries, Entry},
    ledger::error::EcCaughtUpTimeout,
    outbox::{CalaMailboxTables, ObixOutbox, OutboxEventPayload},
    primitives::{EntryId, TransactionId},
};

const CALCULATION_ROLLUP_JOB: JobType = JobType::new("cala.calculation_rollup");

/// Maximum number of collected events (transactions + their entries)
/// folded into a single commit.
const MAX_EVENTS_PER_BATCH: usize = 1_000;

/// Register the calculation rollup and spawn its single instance.
///
/// Must be called **before** [`Jobs::start_poll`]. The returned handle is
/// the ledger's observation point on the rollup.
pub(crate) async fn register_calculation_rollup(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    calculations: &Calculations,
    entries: &Entries,
) -> Result<Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>, CalaFault> {
    outbox
        .register_singleton_subscriber(
            jobs,
            OutboxEventJobConfig::new(CALCULATION_ROLLUP_JOB)
                .with_max_batch_size(MAX_EVENTS_PER_BATCH),
            CalculationRollupHandler {
                calculations: calculations.clone(),
                entries: entries.clone(),
            },
        )
        .await
        .narrow_rejected()
}

/// A transaction pulled from a `TransactionCreated` event, with its outbox
/// sequence. The values are read back through `event`, the shared event the
/// outbox decoded once.
pub(crate) struct PendingTx {
    pub(crate) id: TransactionId,
    pub(crate) sequence: EventSequence,
    pub(crate) event: Arc<PersistentOutboxEvent<OutboxEventPayload>>,
}

impl PendingTx {
    fn transaction(&self) -> &TransactionValues {
        match &self.event.payload {
            Some(OutboxEventPayload::TransactionCreated { transaction }) => transaction,
            _ => unreachable!(
                "PendingTx is only built from a TransactionCreated event in handle_persistent"
            ),
        }
    }

    fn entry_ids(&self) -> &[EntryId] {
        &self.transaction().entry_ids
    }
}

/// The entry carried by a collected `EntryCreated` event.
fn entry_of(event: &PersistentOutboxEvent<OutboxEventPayload>) -> &EntryValues {
    match &event.payload {
        Some(OutboxEventPayload::EntryCreated { entry }) => entry,
        _ => unreachable!(
            "CalculationRollupBatch::entries only ever holds EntryCreated events, pushed in handle_persistent"
        ),
    }
}

/// One batch landing's accumulator; see [`crate::ec_rollup`] for why a
/// transaction's event group may straddle landings and how incomplete
/// groups fall back to a DB read.
#[derive(Default)]
pub(crate) struct CalculationRollupBatch {
    txns: Vec<PendingTx>,
    entries: HashMap<TransactionId, Vec<Arc<PersistentOutboxEvent<OutboxEventPayload>>>>,
}

impl CalculationRollupBatch {
    pub(crate) fn push_tx(&mut self, tx: PendingTx) {
        self.txns.push(tx);
    }

    pub(crate) fn push_entry(&mut self, event: Arc<PersistentOutboxEvent<OutboxEventPayload>>) {
        self.entries
            .entry(entry_of(&event).transaction_id)
            .or_default()
            .push(event);
    }

    /// Entry ids that were *not* collected from the stream in this landing.
    pub(crate) fn missing_entry_ids(&self) -> Vec<EntryId> {
        self.txns
            .iter()
            .flat_map(|tx| {
                let collected: HashSet<EntryId> = self
                    .entries
                    .get(&tx.id)
                    .map(|events| events.iter().map(|e| entry_of(e).id).collect())
                    .unwrap_or_default();
                tx.entry_ids()
                    .iter()
                    .copied()
                    .filter(move |id| !collected.contains(id))
            })
            .collect()
    }

    /// The applier's input in landing order, each transaction's entries
    /// topped up from `fetched` and sorted by entry sequence.
    pub(crate) fn rollup_txns<'a>(
        &'a self,
        fetched: &'a HashMap<EntryId, Entry>,
    ) -> Vec<CalculationRollupTxn<'a>> {
        self.txns
            .iter()
            .map(|tx| {
                let entry_ids = tx.entry_ids();
                let mut entry_values: Vec<&EntryValues> = self
                    .entries
                    .get(&tx.id)
                    .map(|events| events.iter().map(|e| entry_of(e)).collect())
                    .unwrap_or_default();
                if entry_values.len() != entry_ids.len() {
                    entry_values.extend(
                        entry_ids
                            .iter()
                            .filter_map(|id| fetched.get(id))
                            .map(Entry::values),
                    );
                }
                entry_values.sort_by_key(|e| e.sequence);

                CalculationRollupTxn {
                    sequence: u64::from(tx.sequence),
                    transaction: tx.transaction(),
                    entries: entry_values,
                }
            })
            .collect()
    }
}

struct CalculationRollupHandler {
    calculations: Calculations,
    entries: Entries,
}

impl SingletonSubscriber<OutboxEventPayload> for CalculationRollupHandler {
    type Batch = CalculationRollupBatch;

    async fn handle_persistent<'inv>(
        &self,
        ctx: EventCtx<'inv, Self::Batch>,
        event: &EventDelivery<OutboxEventPayload>,
    ) -> Result<Handled<'inv>, Box<dyn std::error::Error + Send + Sync>> {
        match &event.payload {
            Some(OutboxEventPayload::TransactionCreated { transaction }) => {
                let tx = PendingTx {
                    id: transaction.id,
                    sequence: event.sequence,
                    event: event.inner().clone(),
                };
                Ok(ctx.collect_with(|batch| batch.push_tx(tx)))
            }
            Some(OutboxEventPayload::EntryCreated { .. }) => {
                let event = event.inner().clone();
                Ok(ctx.collect_with(|batch| batch.push_entry(event)))
            }
            _ => Ok(ctx.skip()),
        }
    }

    #[tracing::instrument(
        name = "cala_ledger.calculation_rollup.flush",
        skip_all,
        fields(txns_count = batch.txns.len(), deltas_count, evaluation_errors),
        err(level = "warn")
    )]
    async fn flush(
        &self,
        op: &mut FlushOp<'_>,
        batch: Self::Batch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.calculations.lock_handoff_shared_in_op(op).await?;
        if !self.calculations.any_exist_in_op(op).await? {
            return Ok(());
        }
        let missing_ids = batch.missing_entry_ids();
        let fetched = if missing_ids.is_empty() {
            HashMap::new()
        } else {
            self.entries.find_all_in_op(&mut *op, &missing_ids).await?
        };

        let rollup_txns = batch.rollup_txns(&fetched);
        let stats = self
            .calculations
            .apply_rollup_in_op(op, rollup_txns, RollupScope::Tip)
            .await?;
        let span = tracing::Span::current();
        span.record("deltas_count", stats.deltas);
        span.record("evaluation_errors", stats.evaluation_errors);
        Ok(())
    }
}

/// A snapshot of the calculation rollup's position. Every outbox event with
/// sequence ≤ `applied` is folded into calculation series and committed.
///
/// `frontier` is pinned at construction. [`refresh`](Self::refresh) advances
/// `applied` against that same fence, so [`lag`](Self::lag) drains toward it.
/// The calculation rollup's lag is independent of the EC rollup's.
///
/// This fences the tip only: a unit in `BackfillStatus::InProgress` is not
/// covered until its own fence, `Calculations::await_backfill`, passes.
#[derive(Debug, Clone)]
pub struct CalculationRollupStatus {
    /// The calculation rollup's committed checkpoint.
    pub applied: EventSequence,
    /// The outbox frontier pinned when this snapshot was taken.
    pub frontier: EventSequence,
    handle: Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>,
}

impl CalculationRollupStatus {
    pub(crate) fn new(
        applied: EventSequence,
        frontier: EventSequence,
        handle: Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>,
    ) -> Self {
        Self {
            applied,
            frontier,
            handle,
        }
    }

    /// Re-read the committed checkpoint, keeping the pinned `frontier`.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation_rollup_status.refresh",
        skip_all,
        fields(frontier = %self.frontier, applied, lag)
    )]
    pub async fn refresh(&mut self) -> Result<(), CalaFault> {
        self.applied = self.handle.load().await.narrow_rejected()?.checkpoint();

        let span = tracing::Span::current();
        span.record("applied", u64::from(self.applied));
        span.record("lag", self.lag());

        Ok(())
    }

    /// Await the calculation rollup applying everything up to this
    /// snapshot's pinned `frontier`. On `Ok(())` every posting that had been
    /// assigned an outbox sequence when the snapshot was taken is folded
    /// into the calculation series it is covered by.
    ///
    /// `timeout` is mandatory: a wedged rollup surfaces as
    /// [`EcCaughtUpTimeout`], never a silent hang.
    pub async fn await_completion(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), Fail<EcCaughtUpTimeout, lanes!(Transient, Fatal)>> {
        crate::ledger::await_rollup(&self.handle, self.frontier, timeout).await?;
        Ok(())
    }

    /// Outbox positions the calculation rollup has yet to consume.
    pub fn lag(&self) -> u64 {
        u64::from(self.frontier).saturating_sub(u64::from(self.applied))
    }

    pub fn is_caught_up(&self) -> bool {
        self.applied >= self.frontier
    }
}
