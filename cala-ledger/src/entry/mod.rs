mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{new_atomic_operation::*, outbox::*, primitives::DataSource};

pub use entity::*;
use error::*;
use repo::*;

#[derive(Clone)]
pub struct Entries {
    repo: EntryRepo,
    outbox: Outbox,
    _pool: PgPool,
}

impl Entries {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: EntryRepo::new(pool),
            outbox,
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn create_all_in_op(
        &self,
        db: &mut AtomicOperation<'_>,
        entries: Vec<NewEntry>,
    ) -> Result<Vec<EntryValues>, EntryError> {
        let entries = self.repo.create_all_in_op(db.op(), entries).await?;
        db.accumulate(
            entries
                .iter()
                .map(|entry| OutboxEventPayload::EntryCreated {
                    source: DataSource::Local,
                    entry: entry.values().clone(),
                }),
        );
        Ok(entries
            .into_iter()
            .map(|entry| entry.into_values())
            .collect())
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_entry_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: EntryValues,
    ) -> Result<(), EntryError> {
        let mut entry = Entry::import(origin, values);
        self.repo.import(&mut db, origin, &mut entry).await?;
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                entry.events.last_persisted(1).map(|p| &p.event),
                recorded_at,
            )
            .await?;
        Ok(())
    }
}

impl From<&EntryEvent> for OutboxEventPayload {
    fn from(event: &EntryEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            EntryEvent::Imported {
                source,
                values: entry,
            } => OutboxEventPayload::EntryCreated {
                source: *source,
                entry: entry.clone(),
            },
            EntryEvent::Initialized { values: entry } => OutboxEventPayload::EntryCreated {
                source: DataSource::Local,
                entry: entry.clone(),
            },
        }
    }
}
