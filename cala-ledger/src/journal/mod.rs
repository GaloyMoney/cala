mod entity;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, errors::*, outbox::*, primitives::DataSource};

pub use entity::*;
use repo::*;

/// Service for working with `Journal` entities.
#[derive(Clone)]
pub struct Journals {
    repo: JournalRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl Journals {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: JournalRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.journals.create", skip(self))]
    pub async fn create(
        &self,
        new_journal: NewJournal,
    ) -> Result<Journal, OneOf<(UnexpectedDbError,)>> {
        let mut tx = self.pool.begin().await.map_err(UnexpectedDbError)?;
        let EntityUpdate {
            entity: journal,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_journal).await?;

        self.outbox
            .persist_events(tx, journal.events.last_persisted(n_new_events))
            .await?;
        Ok(journal)
    }

    #[instrument(name = "cala_ledger.journals.find_all", skip(self), err)]
    pub async fn find_all(
        &self,
        journal_ids: &[JournalId],
    ) -> Result<HashMap<JournalId, JournalValues>, OneOf<(HydratingEntityError, UnexpectedDbError)>>
    {
        self.repo.find_all(journal_ids).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_journal_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: JournalValues,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let mut journal = Journal::import(origin, values);
        self.repo
            .import(&mut tx, recorded_at, origin, &mut journal)
            .await?;
        self.outbox
            .persist_events_at(tx, journal.events.last_persisted(1), recorded_at)
            .await?;
        Ok(())
    }
}

impl From<&JournalEvent> for OutboxEventPayload {
    fn from(event: &JournalEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            JournalEvent::Imported { source, values } => OutboxEventPayload::JournalCreated {
                source: *source,
                journal: values.clone(),
            },
            JournalEvent::Initialized { values } => OutboxEventPayload::JournalCreated {
                source: DataSource::Local,
                journal: values.clone(),
            },
        }
    }
}
