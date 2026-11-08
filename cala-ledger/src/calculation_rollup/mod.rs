//! Streaming maintenance of calculation effective balance series.
//!
//! One obix [`JoinableSubscriber`] folds each committed transaction's
//! entries into the series of every calculation that covers the entry's
//! account or one of its ancestor sets. It is the sole writer of
//! `cala_calculation_effective_balances` and has its own checkpoint, so the
//! posting path and the EC rollup are unaffected by calculations.
//!
//! ## Shape
//!
//! The **tip** is the live writer: a resident job consuming the outbox in
//! `sequence` order, like [`crate::ec_rollup`]. `TransactionCreated` and
//! `EntryCreated` events are collected into the pending [batch](batch),
//! everything else is skipped. When the batch lands the runner calls
//! [`flush`](JoinableSubscriber::flush) once, **inside the transaction that
//! commits the checkpoint**, so the series writes and the stream position
//! land atomically. Entries are applied straight from the stream when a
//! transaction's whole event group landed in the batch, with a DB read
//! through the flush op as the fallback.
//!
//! A unit created with [`History::Backfill`](crate::calculation::History)
//! starts `InProgress` with its own **catch-up**: the same handler, born at
//! the start of the persisted outbox, folding only that unit
//! ([`RollupScope::Backfill`]). obix ends the catch-up in one join landing
//! under the tip's lock, with [`Scope::Join`]: the catch-up's last batch,
//! the flip to `Complete` at the join sequence and the catch-up's retirement
//! commit together, and no tip landing straddles the join. From then on the
//! tip covers the unit past the join sequence.
//!
//! The catch-up reads the persisted outbox from its start, so it relies on
//! the outbox never being pruned: obix's partition maintainer only creates
//! partitions. PR #795 (outbox archiving) contradicts that assumption for
//! history older than its retention window.
//!
//! ## Coverage
//!
//! The tip applies a calculation to a transaction whose outbox sequence is
//! greater than the calculation's `activation_sequence` (Global) or the
//! target's attachment `activation_sequence` (Attached), and never to a
//! unit still `InProgress`. A CEL evaluation error for one entry is logged
//! and the entry treated as not matching that calculation; it never fails
//! the flush.

mod batch;

use es_entity::errlanes::{lanes, Fail, ResultExt};
use serde::{Deserialize, Serialize};

use job::{JobType, Jobs};
use obix::{
    out::{
        EventDelivery, FlushOp, Handled, InsertOrder, JoinableEventCtx, JoinableHandle,
        JoinableSubscriber, JoinableSubscriberConfig, Scope, Subscription,
    },
    EventSequence,
};

use self::batch::{CalculationRollupBatch, Collected};
use crate::{
    calculation::{Calculations, RollupScope},
    entry::Entries,
    error::CalaFault,
    ledger::error::EcCaughtUpTimeout,
    outbox::{CalaMailboxTables, ObixOutbox, OutboxEventPayload},
    primitives::{AccountId, CalculationId, JournalId},
};

pub(crate) const CALCULATION_ROLLUP_JOB: JobType = JobType::new("cala.calculation_rollup");

/// Maximum number of collected events (transactions + their entries)
/// folded into a single commit, by the tip and by a catch-up alike.
const MAX_EVENTS_PER_BATCH: usize = 1_000;
const MAX_CONCURRENT_CATCH_UPS_PER_PROCESS: usize = 2;

/// The address of the rollup's catch-ups: `Calculations` starts one per
/// backfilled unit.
pub(crate) type BackfillCatchUps =
    JoinableHandle<OutboxEventPayload, BackfillUnit, CalaMailboxTables>;

/// What one catch-up folds: a Global calculation (`account_ids == None`)
/// or the attachments created together by one `attach*` call.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackfillUnit {
    pub calculation_id: CalculationId,
    pub journal_id: JournalId,
    pub account_ids: Option<Vec<AccountId>>,
}

/// Register the calculation rollup: the tip is spawned, the catch-ups are
/// started by `Calculations`.
///
/// Must be called **before** [`Jobs::start_poll`]. The returned handle is
/// the ledger's observation point on the tip.
pub(crate) async fn register_calculation_rollup(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    calculations: &Calculations,
    entries: &Entries,
) -> Result<Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>, CalaFault> {
    let handle = outbox
        .register_joinable_subscriber(
            jobs,
            JoinableSubscriberConfig::new(CALCULATION_ROLLUP_JOB)
                .with_max_batch_size(MAX_EVENTS_PER_BATCH)
                .with_catch_up_max_batch_size(MAX_EVENTS_PER_BATCH)
                .with_max_concurrent_catch_ups_per_process(MAX_CONCURRENT_CATCH_UPS_PER_PROCESS),
            CalculationRollupHandler {
                calculations: calculations.clone(),
                entries: entries.clone(),
            },
        )
        .await?;
    handle.tip()
}

struct CalculationRollupHandler {
    calculations: Calculations,
    entries: Entries,
}

impl JoinableSubscriber<OutboxEventPayload> for CalculationRollupHandler {
    type Unit = BackfillUnit;
    type Batch = CalculationRollupBatch;

    async fn handle<'inv>(
        &self,
        ctx: JoinableEventCtx<'inv, Self::Batch>,
        scope: Scope<'_, Self::Unit>,
        event: &EventDelivery<OutboxEventPayload>,
    ) -> Result<Handled<'inv>, Box<dyn std::error::Error + Send + Sync>> {
        let collected = match scope {
            Scope::Tip => Collected::from_event(event),
            Scope::CatchUp { unit } | Scope::Join { unit, .. } => {
                Collected::from_event(event).filter(|c| c.journal_id() == unit.journal_id)
            }
        };
        match collected {
            Some(collected) => Ok(ctx.collect_with(|batch| batch.push(collected))),
            None => Ok(ctx.skip()),
        }
    }

    #[tracing::instrument(
        name = "cala_ledger.calculation_rollup.flush",
        skip_all,
        fields(scope, txns_count = batch.txns_count(), deltas_count, evaluation_errors),
        err(level = "warn")
    )]
    async fn flush(
        &self,
        op: &mut FlushOp<'_, InsertOrder>,
        scope: Scope<'_, Self::Unit>,
        batch: Self::Batch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let span = tracing::Span::current();
        span.record("scope", tracing::field::debug(&scope));
        let rollup_scope = match scope {
            Scope::Tip => {
                if !self.calculations.any_exist_in_op(op).await? {
                    return Ok(());
                }
                RollupScope::Tip
            }
            Scope::CatchUp { unit } | Scope::Join { unit, .. } => RollupScope::Backfill {
                calculation_id: unit.calculation_id,
                targets: unit.account_ids.as_deref(),
            },
        };
        let stats = batch
            .fold_in_op(op, &self.entries, &self.calculations, rollup_scope)
            .await?;
        span.record("deltas_count", stats.deltas);
        span.record("evaluation_errors", stats.evaluation_errors);
        if let Scope::Join { unit, join } = scope {
            self.calculations
                .complete_backfill_in_op(
                    op,
                    unit.calculation_id,
                    unit.account_ids.as_deref(),
                    u64::from(join),
                )
                .await?;
        }
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
