pub mod error;
mod listener;
mod repo;
pub mod server;

mod event {
    pub use cala_types::outbox::*;
    pub use cala_types::primitives::OutboxEventId;
}

use sqlx::{PgPool, Postgres, Transaction};

use error::*;
pub use event::*;
use listener::*;
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
            .persist_events(&mut tx, events.into_iter().map(Into::into))
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn register_listener(
        &self,
        start_after: Option<EventSequence>,
    ) -> Result<OutboxListener, OutboxError> {
        unimplemented!()
        // let sub = self.event_receiver.resubscribe();
        // let latest_known = self.sequences_for(account_id).await?.read().await.0;
        // let start = start_after.unwrap_or(latest_known);
        // Ok(OutboxListener::new(
        //     self.repo.clone(),
        //     augment.then(|| self.augmenter.clone()),
        //     sub,
        //     account_id,
        //     start,
        //     latest_known,
        //     self.buffer_size,
        // ))
    }
}
