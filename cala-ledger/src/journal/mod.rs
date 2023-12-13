mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use crate::outbox::*;

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
    pub async fn create(&self, new_journal: NewJournal) -> Result<JournalId, JournalError> {
        let mut tx = self.pool.begin().await?;
        let res = self.repo.create_in_tx(&mut tx, new_journal).await?;
        self.outbox.persist_events(tx, res.new_events).await?;
        Ok(res.id)
    }
}

impl From<JournalEvent> for OutboxEventPayload {
    fn from(event: JournalEvent) -> Self {
        match event {
            JournalEvent::Initialized { values: journal } => {
                OutboxEventPayload::JournalCreated { journal }
            }
        }
    }
}
