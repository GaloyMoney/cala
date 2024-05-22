mod listener;
mod repo;
pub mod server;

mod event {
    pub use cala_types::outbox::*;
    pub use cala_types::primitives::OutboxEventId;
}

use chrono::{DateTime, Utc};
use sqlx::{postgres::PgListener, PgPool, Postgres, Transaction};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::broadcast;

use crate::errors::*;

pub use event::*;
pub use listener::*;
use repo::*;

const DEFAULT_BUFFER_SIZE: usize = 100;

#[derive(Clone)]
pub(crate) struct Outbox {
    repo: OutboxRepo,
    _pool: PgPool,
    event_sender: broadcast::Sender<OutboxEvent>,
    event_receiver: Arc<broadcast::Receiver<OutboxEvent>>,
    highest_known_sequence: Arc<AtomicU64>,
    buffer_size: usize,
}

impl Outbox {
    pub(crate) async fn init(pool: &PgPool) -> Result<Self, OneOf<(UnexpectedDbError,)>> {
        let buffer_size = DEFAULT_BUFFER_SIZE;
        let (sender, recv) = broadcast::channel(buffer_size);
        let repo = OutboxRepo::new(pool);
        let highest_known_sequence =
            Arc::new(AtomicU64::from(repo.highest_known_sequence().await?));
        Self::spawn_pg_listener(pool, sender.clone(), Arc::clone(&highest_known_sequence)).await?;
        Ok(Self {
            event_sender: sender,
            event_receiver: Arc::new(recv),
            repo,
            highest_known_sequence,
            _pool: pool.clone(),
            buffer_size,
        })
    }

    pub(crate) async fn persist_events(
        &self,
        tx: Transaction<'_, Postgres>,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        self.persist_events_at(tx, events, None).await
    }

    pub(crate) async fn persist_events_at(
        &self,
        mut tx: Transaction<'_, Postgres>,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
        recorded_at: impl Into<Option<DateTime<Utc>>>,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let recorded_at = recorded_at.into();
        let events = self
            .repo
            .persist_events(&mut tx, recorded_at, events.into_iter().map(Into::into))
            .await?;
        tx.commit()
            .await
            .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;

        let mut new_highest_sequence = EventSequence::BEGIN;
        for event in events {
            new_highest_sequence = event.sequence;
            self.event_sender
                .send(event)
                .expect("Outbox event receiver dropped");
        }
        self.highest_known_sequence
            .fetch_max(u64::from(new_highest_sequence), Ordering::AcqRel);
        Ok(())
    }

    pub async fn register_listener(
        &self,
        start_after: Option<EventSequence>,
    ) -> Result<OutboxListener, OneOf<(UnexpectedDbError,)>> {
        let sub = self.event_receiver.resubscribe();
        let latest_known = EventSequence::from(self.highest_known_sequence.load(Ordering::Relaxed));
        let start = start_after.unwrap_or(latest_known);
        Ok(OutboxListener::new(
            self.repo.clone(),
            sub,
            start,
            latest_known,
            self.buffer_size,
        ))
    }

    async fn spawn_pg_listener(
        pool: &PgPool,
        sender: broadcast::Sender<OutboxEvent>,
        highest_known_sequence: Arc<AtomicU64>,
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        let mut listener = PgListener::connect_with(pool)
            .await
            .map_err(UnexpectedDbError)?;
        listener
            .listen("cala_outbox_events")
            .await
            .map_err(UnexpectedDbError)?;
        tokio::spawn(async move {
            loop {
                if let Ok(notification) = listener.recv().await {
                    if let Ok(event) = serde_json::from_str::<OutboxEvent>(notification.payload()) {
                        let new_highest_sequence = u64::from(event.sequence);
                        highest_known_sequence.fetch_max(new_highest_sequence, Ordering::AcqRel);
                        if sender.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(())
    }
}
