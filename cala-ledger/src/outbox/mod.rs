pub mod error;
mod event;
mod repo;

use sqlx::PgPool;

use event::*;
use repo::*;

#[derive(Clone)]
pub struct Outbox {
    _pool: PgPool,
    repo: OutboxRepo,
}

impl Outbox {
    pub fn new(pool: PgPool) -> Self {
        Self {
            _pool: pool.clone(),
            repo: OutboxRepo::new(pool),
        }
    }

    pub(crate) fn persist_events(
        &self,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) {
        //
    }
}
