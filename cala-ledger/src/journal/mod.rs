mod entity;
pub mod error;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{atomic_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
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
    pub async fn create(&self, new_journal: NewJournal) -> Result<Journal, JournalError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let journal = self.create_in_op(&mut op, new_journal).await?;
        op.commit().await?;
        Ok(journal)
    }

    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_journal: NewJournal,
    ) -> Result<Journal, JournalError> {
        let journal = self.repo.create_in_tx(op.tx(), new_journal).await?;
        op.accumulate(journal.events.last_persisted());
        Ok(journal)
    }

    #[instrument(name = "cala_ledger.journals.find_all", skip(self), err)]
    pub async fn find_all<T: From<Journal>>(
        &self,
        journal_ids: &[JournalId],
    ) -> Result<HashMap<JournalId, T>, JournalError> {
        self.repo.find_all(journal_ids).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_journal_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: JournalValues,
    ) -> Result<(), JournalError> {
        let mut journal = Journal::import(origin, values);
        self.repo
            .import(&mut db, recorded_at, origin, &mut journal)
            .await?;
        self.outbox
            .persist_events_at(db, journal.events.last_persisted(), recorded_at)
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
