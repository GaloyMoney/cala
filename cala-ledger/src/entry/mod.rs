mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{outbox::*, primitives::DataSource};

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

    #[cfg(feature = "import")]
    pub async fn sync_entry_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        values: EntryValues,
    ) -> Result<(), EntryError> {
        let mut entry = Entry::import(origin, values);
        self.repo.import(&mut tx, origin, &mut entry).await?;
        self.outbox
            .persist_events(tx, entry.events.last_persisted(1))
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
