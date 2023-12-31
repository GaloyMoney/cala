mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use crate::{entity::*, outbox::*};

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
    pub fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: JournalRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.journals.create", skip(self))]
    pub async fn create(&self, new_journal: NewJournal) -> Result<Journal, JournalError> {
        let mut tx = self.pool.begin().await?;
        let EntityUpdate {
            entity: journal,
            n_new_events,
        } = self.repo.create_in_tx(&mut tx, new_journal).await?;

        self.outbox
            .persist_events(tx, journal.events.last_persisted(n_new_events))
            .await?;
        Ok(journal)
    }
}

impl From<&JournalEvent> for OutboxEventPayload {
    fn from(event: &JournalEvent) -> Self {
        match event {
            JournalEvent::Initialized { values: journal } => OutboxEventPayload::JournalCreated {
                journal: journal.clone(),
            },
        }
    }
}
