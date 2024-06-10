use sqlx::{PgPool, Postgres, Transaction};

use crate::outbox::*;

pub struct AtomicOperation<'t> {
    tx: Transaction<'t, Postgres>,
    outbox: Outbox,
    accumulated_events: Vec<OutboxEventPayload>,
}

impl<'t> AtomicOperation<'t> {
    pub(crate) async fn init(pool: &PgPool, outbox: &Outbox) -> Result<Self, sqlx::Error> {
        Ok(Self {
            tx: pool.begin().await?,
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
