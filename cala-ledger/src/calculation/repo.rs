use crate::error::CalaFault;
use es_entity::*;
use sqlx::PgPool;

use crate::outbox::OutboxPublisher;

use super::entity::*;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Calculation",
    id(source = caller),
    columns(
        journal_id(ty = "JournalId", update(persist = false)),
        code(ty = "String", update(accessor = "values().code", persist = false)),
        name(
            ty = "String",
            update(accessor = "values().name", persist = false),
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
        name = "calculation.list_for_journal",
        skip_all
    )]
    pub async fn list_for_journal(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
    ) -> Result<Vec<Calculation>, CalaFault> {
        let rows = op
            .into_executor()
            .fetch_all(sqlx::query_as!(
                GenericEvent,
                r#"WITH calculations AS (
              SELECT id, created_at AS entity_created_at
              FROM cala_calculations
              WHERE journal_id = $1
            )
            SELECT c.id as entity_id, e.sequence, e.event, NULL as "context: es_entity::ContextData", e.recorded_at, NULL::jsonb as "forgettable_payload?"
            FROM calculations c
            JOIN cala_calculation_events e ON c.id = e.id
            ORDER BY c.id, e.sequence"#,
                journal_id as JournalId,
            ))
            .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n::<Calculation>(rows, n)?.0;
        Ok(ret)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "calculation.journal_exists",
        skip_all
    )]
    pub async fn journal_exists(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
    ) -> Result<bool, CalaFault> {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM cala_journals WHERE id = $1) AS "exists!""#,
            journal_id as JournalId,
        )
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
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
