use sqlx::{PgPool, Postgres, Transaction};

use crate::outbox::*;

pub struct AtomicOperation<'a> {
    tx: Transaction<'a, Postgres>,
    outbox: Outbox,
    accumulated_events: Vec<OutboxEventPayload>,
}

impl<'a> AtomicOperation<'a> {
    pub(crate) async fn init(pool: &PgPool, outbox: &Outbox) -> Result<Self, sqlx::Error> {
        Ok(Self {
            tx: pool.begin().await?,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        })
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.outbox
            .persist_events(self.tx, self.accumulated_events)
            .await?;
        Ok(())
    }
}
