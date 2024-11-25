use sqlx::{PgPool, Postgres, Transaction};

use crate::outbox::*;

pub struct AtomicOperation<'t> {
    pub(crate) now: chrono::DateTime<chrono::Utc>,

    tx: Transaction<'t, Postgres>,
    outbox: Outbox,
    accumulated_events: Vec<OutboxEventPayload>,
}

impl<'t> AtomicOperation<'t> {
    pub(crate) async fn init(pool: &PgPool, outbox: &Outbox) -> Result<Self, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let now = sqlx::query!("SELECT NOW()")
            .fetch_one(&mut *tx)
            .await?
            .now
            .expect("NOW() is not NULL");
        Ok(Self {
            tx,
            now,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        })
    }

    pub fn tx(&mut self) -> &mut Transaction<'t, Postgres> {
        &mut self.tx
    }

    pub(crate) fn accumulate(
        &mut self,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) {
        self.accumulated_events
            .extend(events.into_iter().map(|e| e.into()))
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        if self.accumulated_events.is_empty() {
            self.tx.commit().await?;
        } else {
            self.outbox
                .persist_events(self.tx, self.accumulated_events)
                .await?;
        }
        Ok(())
    }
}
