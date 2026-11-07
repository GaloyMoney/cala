//! One-off replay of history into a new calculation or attachment batch.
//!
//! A `History::Backfill` unit starts `InProgress` with one member of the
//! `cala.calculation_backfill` keyed subscriber type, born at the start of
//! the persisted outbox and never passivating. Every landing folds its
//! batch with `RollupScope::Backfill`, so only that unit is written, and
//! compares the member's position `P` with the tip's checkpoint `C`. Once
//! `P >= C` it takes the EXCLUSIVE side of the calculation handoff lock,
//! which waits for every in-flight tip flush (each holds the SHARED side),
//! re-reads `C`, folds the gap `(P, C]` from the outbox if the tip moved
//! meanwhile, flips the unit to `Complete` with
//! `activation_sequence = max(P, C)` and finishes: the fold, the flip, the
//! checkpoint and the member's retirement commit in one transaction. The
//! tip covers the unit from the activation on, so every transaction lands
//! in the unit exactly once (§2.2 of the handoff).
//!
//! The member collects every event, not only the ones it folds: a landing
//! is the only place a handoff can be decided, and the tip's checkpoint
//! also moves on events the fold ignores.
//!
//! The replay reads the persisted outbox from its start, so it relies on
//! the outbox never being pruned: obix's partition maintainer only creates
//! partitions. PR #795 (outbox archiving) contradicts that assumption for
//! history older than its retention window.

use es_entity::errlanes::{Fatal, FatalKind, ResultExt};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use std::time::Duration;

use job::{JobType, Jobs};
use obix::{
    out::{
        EventDelivery, FlushOp, FlushOutcome, Handled, InsertOrder, KeyedEventCtx, KeyedSubscriber,
        KeyedSubscriberConfig, PersistentOutboxEvent, Subscription, SubscriptionDef, Subscriptions,
        WakeKey,
    },
    EventSequence,
};

use super::batch::{CalculationRollupBatch, Collected};
use crate::{
    calculation::{Calculations, RollupScope},
    entry::Entries,
    error::{CalaFault, CouldNotDecodeOutboxEvent},
    outbox::{CalaMailboxTables, ObixOutbox, OutboxEventPayload},
    primitives::{AccountId, CalculationId},
};

const CALCULATION_BACKFILL_JOB: JobType = JobType::new("cala.calculation_backfill");
/// Collected events per landing; same figure as the tip's batch.
const MAX_EVENTS_PER_BATCH: usize = 1_000;
const MAX_CONCURRENT_PER_PROCESS: usize = 2;
/// Declared by every member; never consulted, because members never
/// passivate.
pub(crate) const BACKFILL_WAKE_KEY: &str = "cala.calculation_backfill";

es_entity::entity_id! { CalculationBackfillId }

type TipHandle = Subscription<OutboxEventPayload, InsertOrder, CalaMailboxTables>;
pub(crate) type BackfillSubscriptions =
    Subscriptions<CalculationBackfillDef, OutboxEventPayload, CalaMailboxTables>;

/// The unit: a Global calculation (`account_ids == None`) or the
/// attachments created together by one `attach*` call.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackfillUnit {
    pub calculation_id: CalculationId,
    pub account_ids: Option<Vec<AccountId>>,
}

/// Register the backfill subscriber type. Must be called **before**
/// [`Jobs::start_poll`]; the returned capability creates the members.
pub(crate) async fn register_calculation_backfill(
    jobs: &mut Jobs,
    outbox: &ObixOutbox,
    calculations: &Calculations,
    entries: &Entries,
    tip: &TipHandle,
) -> Result<BackfillSubscriptions, CalaFault> {
    outbox
        .register_keyed_subscriber(
            jobs,
            KeyedSubscriberConfig::new(CALCULATION_BACKFILL_JOB)
                .with_linger(Duration::MAX)
                .with_max_batch_size(MAX_EVENTS_PER_BATCH)
                .with_max_concurrent_per_process(MAX_CONCURRENT_PER_PROCESS),
            CalculationBackfillDef {
                outbox: outbox.clone(),
                calculations: calculations.clone(),
                entries: entries.clone(),
                tip: tip.clone(),
            },
        )
        .await
}

pub(crate) struct CalculationBackfillDef {
    outbox: ObixOutbox,
    calculations: Calculations,
    entries: Entries,
    tip: TipHandle,
}

impl SubscriptionDef<OutboxEventPayload> for CalculationBackfillDef {
    type Key = CalculationBackfillId;
    type InstanceConfig = BackfillUnit;
    type Subscriber = CalculationBackfillSubscriber;

    fn wake_keys(
        &self,
        _event: &PersistentOutboxEvent<OutboxEventPayload>,
    ) -> impl IntoIterator<Item = WakeKey> {
        std::iter::empty()
    }

    fn instantiate(&self, _key: Self::Key, unit: Self::InstanceConfig) -> Self::Subscriber {
        CalculationBackfillSubscriber {
            unit,
            outbox: self.outbox.clone(),
            calculations: self.calculations.clone(),
            entries: self.entries.clone(),
            tip: self.tip.clone(),
        }
    }
}

pub(crate) struct CalculationBackfillSubscriber {
    unit: BackfillUnit,
    outbox: ObixOutbox,
    calculations: Calculations,
    entries: Entries,
    tip: TipHandle,
}

impl CalculationBackfillSubscriber {
    async fn tip_checkpoint(&self) -> Result<u64, CalaFault> {
        Ok(u64::from(
            self.tip.load().await.narrow_rejected()?.checkpoint(),
        ))
    }

    /// The events in `(after, through]`, read from the persisted outbox.
    /// `through` is a committed tip checkpoint, so every one of them exists.
    async fn gap(&self, after: u64, through: u64) -> Result<CalculationRollupBatch, CalaFault> {
        let mut listener = Box::pin(
            self.outbox
                .listen_persisted(Some(EventSequence::from(after))),
        );
        let mut batch = CalculationRollupBatch::default();
        let mut last = after;
        while last < through {
            let event = match listener.next().await {
                Some(Ok(delivery)) => delivery,
                Some(Err(undecodable)) => {
                    return Err(CouldNotDecodeOutboxEvent {
                        sequence: undecodable.sequence,
                    }
                    .into())
                }
                None => {
                    return Err(Fatal::new(FatalKind::Invariant)
                        .with_context(
                            "the persisted outbox listener ended below a committed checkpoint",
                        )
                        .into())
                }
            };
            last = u64::from(event.sequence);
            if last > through {
                break;
            }
            if let Some(collected) = Collected::from_event(&event) {
                batch.push(collected);
            }
        }
        Ok(batch)
    }
}

impl KeyedSubscriber<OutboxEventPayload> for CalculationBackfillSubscriber {
    type Batch = CalculationRollupBatch;

    async fn handle<'inv>(
        &self,
        ctx: KeyedEventCtx<'inv, Self::Batch>,
        event: &EventDelivery<OutboxEventPayload>,
    ) -> Result<Handled<'inv>, Box<dyn std::error::Error + Send + Sync>> {
        let collected = Collected::from_event(event);
        Ok(ctx.collect_with(|batch| {
            if let Some(collected) = collected {
                batch.push(collected);
            }
        }))
    }

    #[tracing::instrument(
        name = "cala_ledger.calculation_backfill.flush",
        skip_all,
        fields(
            calculation_id = %self.unit.calculation_id,
            txns_count = batch.txns_count(),
            deltas_count,
            evaluation_errors,
            position,
            checkpoint,
            gap_txns_count
        ),
        err(level = "warn")
    )]
    async fn flush(
        &self,
        op: &mut FlushOp<'_, InsertOrder>,
        batch: Self::Batch,
    ) -> Result<FlushOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let calculation_id = self.unit.calculation_id;
        let targets = self.unit.account_ids.as_deref();
        let stats = batch
            .fold_in_op(
                op,
                &self.entries,
                &self.calculations,
                RollupScope::Backfill {
                    calculation_id,
                    targets,
                },
            )
            .await?;
        let span = tracing::Span::current();
        span.record("deltas_count", stats.deltas);
        span.record("evaluation_errors", stats.evaluation_errors);

        let position = u64::from(op.position());
        span.record("position", position);
        if position < self.tip_checkpoint().await? {
            return Ok(FlushOutcome::Continue);
        }
        self.calculations.lock_handoff_exclusive_in_op(op).await?;
        let checkpoint = self.tip_checkpoint().await?;
        span.record("checkpoint", checkpoint);
        if position < checkpoint {
            let gap = self.gap(position, checkpoint).await?;
            span.record("gap_txns_count", gap.txns_count());
            gap.fold_in_op(
                op,
                &self.entries,
                &self.calculations,
                RollupScope::Backfill {
                    calculation_id,
                    targets,
                },
            )
            .await?;
        }
        self.calculations
            .complete_backfill_in_op(op, calculation_id, targets, position.max(checkpoint))
            .await?;
        Ok(FlushOutcome::Finish)
    }
}
