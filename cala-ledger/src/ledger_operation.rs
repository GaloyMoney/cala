use sqlx::PgPool;
use tracing::instrument;

use es_entity::{DbOp, DbOpWithTime};

use crate::outbox::*;

pub struct LedgerOperation<'t> {
    db_op: DbOpWithTime<'t>,
    outbox: Outbox,
    accumulated_events: Vec<OutboxEventPayload>,
}

impl<'t> LedgerOperation<'t> {
    #[instrument(name = "ledger_operation.init", skip_all, err)]
    pub(crate) async fn init(
        pool: &PgPool,
        outbox: &Outbox,
    ) -> Result<LedgerOperation<'static>, sqlx::Error> {
        let db_op = DbOp::init(pool).await?.with_db_time().await?;
        Ok(LedgerOperation {
            db_op,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        })
    }

    pub(crate) fn new(db_op: DbOpWithTime<'t>, outbox: &Outbox) -> Self {
        Self {
            db_op,
            outbox: outbox.clone(),
            accumulated_events: Vec::new(),
        }
    }

    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.db_op.now()
    }

    pub async fn begin(&mut self) -> Result<DbOpWithTime<'_>, sqlx::Error> {
        self.db_op.begin().await
    }

    pub(crate) fn accumulate(
        &mut self,
        events: impl IntoIterator<Item = impl Into<OutboxEventPayload>>,
    ) {
        self.accumulated_events
            .extend(events.into_iter().map(|e| e.into()))
    }

    #[instrument(name = "ledger_operation.commit", skip(self), fields(events_count = self.accumulated_events.len()), err)]
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        let tx = sqlx::Transaction::from(self.db_op);
        if self.accumulated_events.is_empty() {
            tx.commit().await?;
        } else {
            self.outbox
                .persist_events(tx, self.accumulated_events)
                .await?;
        }
        Ok(())
    }
}

impl<'t> es_entity::AtomicOperation for LedgerOperation<'t> {
    fn now(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        Some(self.now())
    }

    fn as_executor(&mut self) -> &mut sqlx::PgConnection {
        self.db_op.as_executor()
    }
}
