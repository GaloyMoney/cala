use es_entity::*;
use sqlx::PgPool;

#[cfg(feature = "import")]
use tracing::instrument;

use crate::{outbox::OutboxPublisher, primitives::DataSourceId};

use super::{entity::*, error::JournalError};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Journal",
    err = "JournalError",
    columns(
        name(ty = "String", update(accessor = "values().name")),
        code(ty = "Option<String>", update(accessor = "values().code")),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct JournalRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl JournalRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    #[cfg(feature = "import")]
    #[instrument(name = "journal.import_in_op", skip_all, err(level = "warn"))]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperationWithTime,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_journals (data_source_id, id, name, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            journal.values().id as JournalId,
            journal.values().name,
            recorded_at
        )
        .execute(op.as_executor())
        .await?;
        let n_events = self.persist_events(op, journal.events_mut()).await?;
        self.publish(op, journal, journal.events().last_persisted(n_events))
            .await?;

        Ok(())
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Journal,
        new_events: es_entity::LastPersisted<'_, JournalEvent>,
    ) -> Result<(), JournalError> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
