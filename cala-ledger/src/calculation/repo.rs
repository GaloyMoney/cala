use crate::error::CalaFault;
use es_entity::*;
use sqlx::PgPool;

use crate::outbox::{OutboxEventPayload, OutboxPublisher};

use super::entity::*;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Calculation",
    id(source = caller),
    columns(
        journal_id(
            ty = "JournalId",
            list_for(by(created_at), find_all_for),
            update(persist = false, accessor = "values().journal_id")
        ),
        code(ty = "String", update(accessor = "values().code", persist = false)),
        name(
            ty = "String",
            update(accessor = "values().name", persist = false),
            find_by = false
        ),
        backfill_complete(
            ty = "bool",
            create(accessor = "backfill_complete()"),
            update(accessor = "backfill_complete()"),
            find_by = false
        ),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct CalculationRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl CalculationRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.any_exist_in_op",
        skip_all
    )]
    pub async fn any_exist_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
    ) -> Result<bool, CalaFault> {
        let exists =
            sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM cala_calculations) AS "exists!""#)
                .fetch_one(op.as_executor())
                .await?;
        Ok(exists)
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Calculation,
        new_events: es_entity::LastPersisted<'_, CalculationEvent>,
    ) -> Result<(), CalaFault> {
        let calculation_id = entity.id;
        let payloads = new_events.map(|persisted| match &persisted.event {
            CalculationEvent::Initialized { values } => OutboxEventPayload::CalculationCreated {
                calculation: values.clone(),
            },
            CalculationEvent::BackfillCompleted {
                activation_sequence,
            } => OutboxEventPayload::CalculationBackfillCompleted {
                calculation_id,
                account_ids: None,
                activation_sequence: *activation_sequence,
            },
        });
        self.publisher.publish_all(op, payloads).await?;
        Ok(())
    }
}
