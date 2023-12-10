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
        tx: Transaction<'_, Postgres>,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) -> Result<(), OutboxError> {
        //
        tx.commit().await?;
        Ok(())
    }
}
