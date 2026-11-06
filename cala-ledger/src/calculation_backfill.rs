//! One-off replay of history into a new calculation or attachment batch.
//!
//! A `History::Backfill` unit starts `InProgress` with one
//! `cala_calculation_backfills` row and one job per row. The job replays
//! the persisted outbox from the row's `cursor` in chunks of
//! [`CHUNK_EVENTS`], folding each chunk with `RollupScope::Backfill` so
//! only that unit is written, and chases the tip's checkpoint until the
//! remaining distance is at most [`HANDOFF_THRESHOLD`] or
//! [`MAX_CHASE_ROUNDS`] rounds have run. The handoff then takes the
//! EXCLUSIVE side of the calculation handoff lock, which waits for every
//! in-flight tip flush (each holds the SHARED side), re-reads the
//! checkpoint `C`, replays the remainder in the same operation, flips the
//! unit to `Complete` with `activation_sequence = C` and commits. The
//! tip's next flush covers the unit from `C + 1` on, so every transaction
//! lands in the unit exactly once (§2.2 of the handoff).
//!
//! The replay reads the persisted outbox from the row's cursor, so it
//! relies on the outbox never being pruned: obix's partition maintainer
//! only creates partitions. PR #795 (outbox archiving) contradicts that
//! assumption for history older than its retention window.

use async_trait::async_trait;
use es_entity::errlanes::{Fatal, FatalKind, ResultExt};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use job::{CurrentJob, Job, JobCompletion, JobInitializer, JobRunner, JobSpawner, JobType};
use obix::{
    out::{InsertOrder, PersistentOutboxEvent, PersistentOutboxListener, Subscription},
    EventSequence,
};

use crate::{
    calculation::{CalculationBackfill, Calculations, RollupScope},
    calculation_rollup::{CalculationRollupBatch, PendingTx},
    entry::Entries,
    error::{CalaFault, CouldNotDecodeOutboxEvent},
    outbox::{CalaMailboxTables, ObixOutbox, OutboxEventPayload},
    primitives::CalculationBackfillId,
};

const CALCULATION_BACKFILL_JOB: JobType = JobType::new("cala.calculation_backfill");
/// Collected events per chunk op; same figure as the tip's batch.
const CHUNK_EVENTS: usize = 1_000;
/// Remaining distance to the tip at or below which the handoff runs (B3).
const HANDOFF_THRESHOLD: u64 = 2_000;
/// Chase rounds before the handoff is forced regardless of distance.
const MAX_CHASE_ROUNDS: u32 = 20;
/// Pause between chase rounds while the tip's checkpoint does not move.
const IDLE_ROUND_PAUSE: Duration = Duration::from_millis(250);

type TipHandle = Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackfillJobConfig {
    pub backfill_id: CalculationBackfillId,
}

pub(crate) struct CalculationBackfillInitializer {
    calculations: Calculations,
    entries: Entries,
    outbox: ObixOutbox,
    tip: TipHandle,
}

impl CalculationBackfillInitializer {
    pub(crate) fn new(
        calculations: &Calculations,
        entries: &Entries,
        outbox: &ObixOutbox,
        tip: &TipHandle,
    ) -> Self {
        Self {
            calculations: calculations.clone(),
            entries: entries.clone(),
            outbox: outbox.clone(),
            tip: tip.clone(),
        }
    }
}

impl JobInitializer for CalculationBackfillInitializer {
    type Config = BackfillJobConfig;

    fn job_type(&self) -> JobType {
        CALCULATION_BACKFILL_JOB
    }

    fn max_concurrent_per_process(&self) -> Option<usize> {
        Some(2)
    }

    fn init(
        &self,
        job: &Job,
        _spawner: JobSpawner<Self::Config>,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(CalculationBackfillRunner {
            config: job.config::<BackfillJobConfig>()?,
            calculations: self.calculations.clone(),
            entries: self.entries.clone(),
            outbox: self.outbox.clone(),
            tip: self.tip.clone(),
        }))
    }
}

struct CalculationBackfillRunner {
    config: BackfillJobConfig,
    calculations: Calculations,
    entries: Entries,
    outbox: ObixOutbox,
    tip: TipHandle,
}

/// Where the run is, for the error log and the span.
#[derive(Default)]
struct Progress {
    cursor: u64,
    bound: u64,
    rounds: u32,
    chunks: usize,
    deltas: usize,
    evaluation_errors: usize,
    handoff_span: u64,
}

#[async_trait]
impl JobRunner for CalculationBackfillRunner {
    #[tracing::instrument(
        name = "cala_ledger.calculation_backfill.run",
        skip_all,
        fields(
            backfill_id = %self.config.backfill_id,
            calculation_id,
            rounds,
            chunks,
            deltas,
            evaluation_errors,
            handoff_span
        ),
        err(level = "warn")
    )]
    async fn run(
        &self,
        mut current_job: CurrentJob,
    ) -> Result<JobCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let Some(backfill) = self
            .calculations
            .find_open_backfill(self.config.backfill_id)
            .await?
        else {
            return Ok(JobCompletion::Complete);
        };
        let span = tracing::Span::current();
        span.record(
            "calculation_id",
            tracing::field::display(backfill.calculation_id),
        );

        let mut progress = Progress {
            cursor: backfill.cursor,
            ..Progress::default()
        };
        let result = self
            .execute(&mut current_job, &backfill, &mut progress)
            .await;
        span.record("rounds", progress.rounds);
        span.record("chunks", progress.chunks);
        span.record("deltas", progress.deltas);
        span.record("evaluation_errors", progress.evaluation_errors);
        span.record("handoff_span", progress.handoff_span);
        if result.is_err() {
            tracing::error!(
                backfill_id = %backfill.id,
                calculation_id = %backfill.calculation_id,
                cursor = progress.cursor,
                bound = progress.bound,
                "calculation backfill run failed; the job retries from the persisted cursor"
            );
        }
        Ok(result?)
    }
}

impl CalculationBackfillRunner {
    /// The §2.3 loop: chase rounds in their own operations, then the
    /// handoff in one operation under the EXCLUSIVE lock.
    async fn execute(
        &self,
        current_job: &mut CurrentJob,
        backfill: &CalculationBackfill,
        progress: &mut Progress,
    ) -> Result<JobCompletion, CalaFault> {
        let targets = backfill.account_ids.as_deref();
        let scope = RollupScope::Backfill {
            calculation_id: backfill.calculation_id,
            targets,
        };
        let mut replay = Replay::new(
            self.outbox
                .listen_persisted(Some(EventSequence::from(progress.cursor))),
            progress.cursor,
        );
        let mut previous_bound: Option<u64> = None;

        loop {
            let bound = self.tip_checkpoint().await?;
            progress.bound = bound;
            let distance = bound.saturating_sub(progress.cursor);
            let chase = progress.rounds < 1
                || (distance > HANDOFF_THRESHOLD && progress.rounds < MAX_CHASE_ROUNDS);
            if !chase {
                break;
            }
            while let Some(chunk) = replay.next_chunk(bound).await? {
                let mut op = current_job.begin_op().await?;
                self.fold_chunk(&mut op, &chunk, scope, progress).await?;
                self.calculations
                    .advance_backfill_cursor_in_op(&mut op, backfill.id, chunk.end)
                    .await?;
                op.commit().await?;
                progress.cursor = chunk.end;
                if current_job.is_shutdown_requested() {
                    return Ok(JobCompletion::RescheduleNow);
                }
            }
            progress.cursor = bound;
            progress.rounds += 1;
            if previous_bound == Some(bound) {
                tokio::time::sleep(IDLE_ROUND_PAUSE).await;
            }
            previous_bound = Some(bound);
        }

        let mut op = current_job.begin_op().await?;
        self.calculations
            .lock_handoff_exclusive_in_op(&mut op)
            .await?;
        let bound = self.tip_checkpoint().await?;
        progress.bound = bound;
        progress.handoff_span = bound.saturating_sub(progress.cursor);
        while let Some(chunk) = replay.next_chunk(bound).await? {
            self.fold_chunk(&mut op, &chunk, scope, progress).await?;
            progress.cursor = chunk.end;
        }
        self.calculations
            .complete_backfill_in_op(&mut op, backfill, bound)
            .await?;
        progress.cursor = bound;
        Ok(JobCompletion::CompleteWithOp(op))
    }

    async fn tip_checkpoint(&self) -> Result<u64, CalaFault> {
        Ok(u64::from(
            self.tip.load().await.narrow_rejected()?.checkpoint(),
        ))
    }

    async fn fold_chunk(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        chunk: &Chunk,
        scope: RollupScope<'_>,
        progress: &mut Progress,
    ) -> Result<(), CalaFault> {
        let missing_ids = chunk.batch.missing_entry_ids();
        let fetched = if missing_ids.is_empty() {
            HashMap::new()
        } else {
            self.entries.find_all_in_op(&mut *op, &missing_ids).await?
        };
        let txns = chunk.batch.rollup_txns(&fetched);
        let stats = self
            .calculations
            .apply_rollup_in_op(op, txns, scope)
            .await?;
        progress.chunks += 1;
        progress.deltas += stats.deltas;
        progress.evaluation_errors += stats.evaluation_errors;
        Ok(())
    }
}

/// The collected events of one chunk and the sequence the cursor moves to
/// once they are applied.
struct Chunk {
    batch: CalculationRollupBatch,
    end: u64,
}

/// Reads the persisted outbox a chunk at a time, never pulling a delivery
/// the current bound does not cover more than once: a delivery above the
/// bound is kept for the next call.
struct Replay {
    listener: Pin<Box<PersistentOutboxListener<OutboxEventPayload>>>,
    /// Sequence of the last delivery taken from the listener.
    pulled: u64,
    held: Option<Arc<PersistentOutboxEvent<OutboxEventPayload>>>,
}

impl Replay {
    /// `listener` starts after `cursor`.
    fn new(listener: PersistentOutboxListener<OutboxEventPayload>, cursor: u64) -> Self {
        Self {
            listener: Box::pin(listener),
            pulled: cursor,
            held: None,
        }
    }

    /// The next chunk of `(cursor, bound]`, `None` once the bound is
    /// consumed. A chunk closes at [`CHUNK_EVENTS`] collected events or at
    /// the bound, whichever comes first.
    async fn next_chunk(&mut self, bound: u64) -> Result<Option<Chunk>, CalaFault> {
        let mut batch = CalculationRollupBatch::default();
        let mut collected = 0usize;
        let mut consumed: Option<u64> = None;
        let mut exhausted = true;
        loop {
            let event = match self.held.take() {
                Some(event) if u64::from(event.sequence) <= bound => event,
                Some(event) => {
                    self.held = Some(event);
                    break;
                }
                None if self.pulled < bound => {
                    let event = self.pull().await?;
                    self.pulled = self.pulled.max(u64::from(event.sequence));
                    if u64::from(event.sequence) > bound {
                        self.held = Some(event);
                        break;
                    }
                    event
                }
                None => break,
            };
            consumed = Some(u64::from(event.sequence));
            match &event.payload {
                Some(OutboxEventPayload::TransactionCreated { transaction }) => {
                    batch.push_tx(PendingTx {
                        id: transaction.id,
                        sequence: event.sequence,
                        event: event.clone(),
                    });
                    collected += 1;
                }
                Some(OutboxEventPayload::EntryCreated { .. }) => {
                    batch.push_entry(event.clone());
                    collected += 1;
                }
                _ => {}
            }
            if collected >= CHUNK_EVENTS {
                exhausted = false;
                break;
            }
        }
        Ok(consumed.map(|last| Chunk {
            batch,
            end: if exhausted { bound } else { last },
        }))
    }

    async fn pull(&mut self) -> Result<Arc<PersistentOutboxEvent<OutboxEventPayload>>, CalaFault> {
        match self.listener.next().await {
            Some(Ok(delivery)) => Ok(delivery.into_inner()),
            Some(Err(undecodable)) => Err(CouldNotDecodeOutboxEvent {
                sequence: undecodable.sequence,
            }
            .into()),
            None => Err(Fatal::new(FatalKind::Invariant)
                .with_context("the persisted outbox listener ended below a committed checkpoint")
                .into()),
        }
    }
}
