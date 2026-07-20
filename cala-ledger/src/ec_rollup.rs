//! Streaming rollup of eventually-consistent (EC) account-set balances.
//!
//! A single long-lived [`job`] consumes the obix outbox in `sequence`
//! order and rolls each committed transaction's leaf-entry deltas up into
//! its ancestor **EC** account sets — incrementally and bounded. This
//! replaces the periodic pull/batch `recalculate_balances_deep` as the
//! steady-state mechanism (which could OOM a Postgres backend by replaying
//! a whole set's history in one transaction). Work here is proportional to
//! *new* activity and every commit is size-bounded.
//!
//! ## Why a custom `JobRunner` (not obix's managed `register_event_handler`)
//!
//! obix's `OutboxEventJobRunner` opens a fresh op, calls the handler,
//! advances the cursor and **commits once per event** — it owns the commit
//! boundary and the cursor, so a handler cannot batch/coalesce. We write
//! our own runner over `outbox.listen_all(cursor)` and own the
//! batch/commit/cursor while still getting obix's gapless, in-order,
//! DB-backfilled delivery.
//!
//! ## Correctness
//!
//! - **Exactly-once DB effect.** The applier *adds* deltas (it is not
//!   idempotent), so the cursor is advanced in the **same transaction** as
//!   the rollup writes. A mid-batch crash rolls back both; on restart
//!   `listen_all` re-delivers from the last committed cursor.
//! - **Single writer.** Registered with `spawn_unique`, so exactly one
//!   instance runs cluster-wide — no streaming-vs-streaming contention.
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

use async_trait::async_trait;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use job::{
    CurrentJob, Job, JobCompletion, JobId, JobInitializer, JobRunner, JobSpawner, JobType, Jobs,
    RetrySettings,
};
use obix::{out::PersistentOutboxEvent, EventSequence};

use chrono::{DateTime, NaiveDate, Utc};

use crate::{
    balance::Balances,
    entry::Entries,
    outbox::{ObixOutbox, OutboxEventPayload},
    primitives::{JournalId, TransactionId},
};

const EC_BALANCE_ROLLUP_JOB: JobType = JobType::new("cala.ec_balance_rollup");

/// Maximum number of transactions folded into a single commit. Bounds
/// per-transaction memory/WAL/lock hold-time. The per-statement insert is
/// additionally sub-chunked inside `insert_new_snapshots`.
const MAX_TXNS_PER_BATCH: usize = 1_000;

/// Persisted position in the outbox `sequence`. Advanced atomically with
/// the rollup writes, so it is the single source of truth for "how far the
/// stream has been applied".
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct EcRollupCursor {
    sequence: EventSequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct EcRollupJobConfig;

/// Register the streaming EC-balance rollup and spawn its single instance.
///
/// Must be called **before** [`Jobs::start_poll`] (`add_initializer`
/// panics once polling has started). Idempotent via `spawn_unique`.
pub(crate) async fn spawn_ec_balance_rollup_job(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    balances: &Balances,
    entries: &Entries,
) -> Result<(), job::error::JobError> {
    let spawner = jobs.add_initializer(EcBalanceRollupInitializer::new(outbox, balances, entries));
    spawner
        .spawn_unique(JobId::new(), EcRollupJobConfig)
        .await?;
    Ok(())
}

struct EcBalanceRollupInitializer {
    outbox: ObixOutbox,
    balances: Balances,
    entries: Entries,
}

impl EcBalanceRollupInitializer {
    fn new(outbox: &ObixOutbox, balances: &Balances, entries: &Entries) -> Self {
        Self {
            outbox: outbox.clone(),
            balances: balances.clone(),
            entries: entries.clone(),
        }
    }
}

impl JobInitializer for EcBalanceRollupInitializer {
    type Config = EcRollupJobConfig;

    fn job_type(&self) -> JobType {
        EC_BALANCE_ROLLUP_JOB
    }

    fn retry_on_error_settings(&self) -> RetrySettings {
        RetrySettings::repeat_indefinitely()
    }

    fn init(
        &self,
        _job: &Job,
        _spawner: JobSpawner<Self::Config>,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        Ok(Box::new(EcBalanceRollupRunner {
            outbox: self.outbox.clone(),
            balances: self.balances.clone(),
            entries: self.entries.clone(),
        }))
    }
}

/// A transaction pulled from a `TransactionCreated` event, carrying just
/// what the rollup needs; the entries themselves are loaded from the
/// (already-committed) ledger by id.
struct PendingTx {
    id: TransactionId,
    journal_id: JournalId,
    effective: NaiveDate,
    created_at: DateTime<Utc>,
}

struct EcBalanceRollupRunner {
    outbox: ObixOutbox,
    balances: Balances,
    entries: Entries,
}

#[async_trait]
impl JobRunner for EcBalanceRollupRunner {
    #[tracing::instrument(name = "cala_ledger.ec_rollup.run", skip_all)]
    async fn run(
        &self,
        mut current_job: CurrentJob,
    ) -> Result<JobCompletion, Box<dyn std::error::Error>> {
        let mut cursor = current_job
            .execution_state::<EcRollupCursor>()?
            .unwrap_or_default();
        // cala only publishes persistent events, so listen to those directly.
        let mut stream = self.outbox.listen_persisted(Some(cursor.sequence));

        loop {
            // Block for the first event of a batch, yielding on shutdown.
            let first = tokio::select! {
                biased;
                _ = current_job.shutdown_requested() => {
                    return Ok(JobCompletion::RescheduleNow);
                }
                event = stream.next() => event,
            };
            let Some(first) = first else {
                // The live listener does not normally terminate; treat a
                // closed stream as "reschedule and reconnect".
                return Ok(JobCompletion::RescheduleNow);
            };

            let mut txns: Vec<PendingTx> = Vec::new();
            let mut last_seq: Option<EventSequence> = None;
            Self::consume_event(first, &mut txns, &mut last_seq);

            // Greedily drain whatever else is already available (without
            // blocking) so bursts coalesce into one commit, bounded by
            // MAX_TXNS_PER_BATCH.
            while txns.len() < MAX_TXNS_PER_BATCH {
                match stream.next().now_or_never() {
                    Some(Some(event)) => Self::consume_event(event, &mut txns, &mut last_seq),
                    Some(None) => break,
                    None => break,
                }
            }

            let mut op =
                es_entity::DbOp::init_with_clock(current_job.pool(), current_job.clock()).await?;
            for tx in &txns {
                self.apply_transaction(&mut op, tx).await?;
            }
            if let Some(seq) = last_seq {
                cursor.sequence = seq;
                current_job
                    .update_execution_state_in_op(&mut op, &cursor)
                    .await?;
            }
            op.commit().await?;
        }
    }
}

impl EcBalanceRollupRunner {
    #[tracing::instrument(
        name = "cala_ledger.ec_rollup.apply_transaction",
        skip(self, op, tx),
        fields(transaction_id = %tx.id),
        err(level = "warn")
    )]
    async fn apply_transaction(
        &self,
        op: &mut es_entity::DbOp<'_>,
        tx: &PendingTx,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Entries were committed atomically with the TransactionCreated
        // event, so they are visible; read them by transaction id.
        let entries = self
            .entries
            .list_for_transaction_id(tx.id)
            .await?
            .into_iter()
            .map(|entry| entry.into_values())
            .collect::<Vec<_>>();

        self.balances
            .apply_ec_rollup_in_op(op, tx.journal_id, entries, tx.effective, tx.created_at)
            .await?;
        Ok(())
    }

    /// Advance the batch's high-watermark for every persistent event (so the
    /// cursor also moves past non-`TransactionCreated` events we skip), and
    /// enqueue the ones that carry a new transaction.
    fn consume_event(
        event: Arc<PersistentOutboxEvent<OutboxEventPayload>>,
        txns: &mut Vec<PendingTx>,
        last_seq: &mut Option<EventSequence>,
    ) {
        *last_seq = Some(event.sequence);
        if let Some(OutboxEventPayload::TransactionCreated { transaction }) = &event.payload {
            txns.push(PendingTx {
                id: transaction.id,
                journal_id: transaction.journal_id,
                effective: transaction.effective,
                created_at: transaction.created_at,
            });
        }
    }
}
