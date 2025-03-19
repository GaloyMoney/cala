use sqlx::{PgPool, Postgres, Transaction};

use es_entity::DbOp;

use crate::outbox::*;

pub struct LedgerOperation<'t> {
    db_op: DbOp<'t>,
    outbox: Outbox,
    accumulated_events: Vec<OutboxEventPayload>,
}

impl<'t> LedgerOperation<'t> {
    pub(crate) async fn init(pool: &PgPool, outbox: &Outbox) -> Result<Self, sqlx::Error> {
        let db_op = DbOp::init(pool).await?;
        Ok(Self {
            db_op,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        })
    }

    pub(crate) fn new(db_op: DbOp<'t>, outbox: &Outbox) -> Self {
        Self {
            db_op,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        }
    }

    pub fn tx(&mut self) -> &mut Transaction<'t, Postgres> {
        self.db_op.tx()
    }

    pub fn op(&mut self) -> &mut DbOp<'t> {
        &mut self.db_op
    }

    pub(crate) fn accumulate(
        &mut self,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) {
        self.accumulated_events
            .extend(events.into_iter().map(|e| e.into()))
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        let tx = self.db_op.into_tx();
        if self.accumulated_events.is_empty() {
            tx.commit().await?;
        } else {
            self.outbox
                .persist_events(tx, self.accumulated_events)
                .await?;
        }
        Ok(())
    }

    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        let tx = self.db_op.into_tx();
        tx.rollback().await?;
        Ok(())
    }
}
