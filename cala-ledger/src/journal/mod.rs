mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{new_atomic_operation::*, outbox::*, primitives::DataSource};

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
        db: &mut AtomicOperation<'_>,
        new_journal: NewJournal,
    ) -> Result<Journal, JournalError> {
        let journal = self.repo.create_in_op(db.op(), new_journal).await?;
        db.accumulate(journal.events.last_persisted(1).map(|p| &p.event));
        Ok(journal)
    }

    #[instrument(name = "cala_ledger.journals.find_all", skip(self), err)]
    pub async fn find_all<T: From<Journal>>(
        &self,
        journal_ids: &[JournalId],
    ) -> Result<HashMap<JournalId, T>, JournalError> {
        self.repo.find_all(journal_ids).await
    }

    #[instrument(name = "cala_ledger.journals.find_by_id", skip(self), err)]
    pub async fn find(&self, journal_id: JournalId) -> Result<Journal, JournalError> {
        self.repo.find_by_id(journal_id).await
    }

    #[instrument(name = "cala_ledger.journals.persist", skip(self, journal))]
    pub async fn persist(&self, journal: &mut Journal) -> Result<(), JournalError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, journal).await?;
        op.commit().await?;
        Ok(())
    }

    pub async fn persist_in_op(
        &self,
        db: &mut AtomicOperation<'_>,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        self.repo.update_in_op(db.op(), journal).await?;
        db.accumulate(journal.events.last_persisted(1).map(|p| &p.event));
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_journal_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: JournalValues,
    ) -> Result<(), JournalError> {
        let mut journal = Journal::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut journal)
            .await?;
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                journal.events.last_persisted(1).map(|p| &p.event),
                recorded_at,
            )
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_journal_update(
        &self,
        mut db: es_entity::DbOp<'_>,
        values: JournalValues,
        fields: Vec<String>,
    ) -> Result<(), JournalError> {
        let mut journal = self.repo.find_by_id(values.id).await?;
        journal.update((values, fields));
        self.repo.update_in_op(&mut db, &mut journal).await?;
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                journal.events.last_persisted(1).map(|p| &p.event),
                recorded_at,
            )
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
            JournalEvent::Updated { values, fields } => OutboxEventPayload::JournalUpdated {
                source: DataSource::Local,
                journal: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
