pub mod error;
mod event;
mod repo;

use sqlx::{PgPool, Postgres, Transaction};

use error::*;
pub use event::*;
use repo::*;

#[derive(Clone)]
pub struct Outbox {
    repo: OutboxRepo,
    _pool: PgPool,
}

impl Outbox {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: OutboxRepo::new(pool),
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn persist_events(
        &self,
        mut tx: Transaction<'_, Postgres>,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) -> Result<(), OutboxError> {
        self.repo
            .persist_events::<WithoutAugmentation>(&mut tx, events)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
