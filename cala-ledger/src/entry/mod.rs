mod entity;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{errors::*, outbox::*, primitives::DataSource};

pub use entity::*;
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

    pub(crate) async fn create_all(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entries: Vec<NewEntry>,
    ) -> Result<(Vec<EntryValues>, Vec<OutboxEventPayload>), OneOf<(UnexpectedDbError,)>> {
        let entries = self.repo.create_all(tx, entries).await?;
        let events = entries
            .iter()
            .map(|values| OutboxEventPayload::EntryCreated {
                source: DataSource::Local,
                entry: values.clone(),
            })
            .collect();
        Ok((entries, events))
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_entry_creation(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: EntryValues,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let mut entry = Entry::import(origin, values);
        self.repo
            .import(&mut tx, recorded_at, origin, &mut entry)
            .await?;
        self.outbox
            .persist_events_at(tx, entry.events.last_persisted(1), recorded_at)
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
