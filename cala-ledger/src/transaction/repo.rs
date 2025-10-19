use es_entity::*;
use sqlx::PgPool;

use crate::primitives::*;

use super::{entity::*, error::TransactionError};

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Transaction",
    err = "TransactionError",
    columns(
        external_id(ty = "Option<String>", update(persist = false)),
        correlation_id(ty = "String", update(persist = false)),
        journal_id(ty = "JournalId", update(persist = false)),
        tx_template_id(ty = "TxTemplateId", update(persist = false), list_for),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
        effective(ty = "chrono::NaiveDate", update(persist = false)),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
pub(super) struct TransactionRepo {
    pool: PgPool,
}

impl TransactionRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[cfg(feature = "import")]
    pub async fn import_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        origin: DataSourceId,
        transaction: &mut Transaction,
    ) -> Result<(), TransactionError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_transactions (data_source_id, id, journal_id, tx_template_id, external_id, correlation_id, created_at, effective)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            origin as DataSourceId,
            transaction.values().id as TransactionId,
            transaction.values().journal_id as JournalId,
            transaction.values().tx_template_id as TxTemplateId,
            transaction.values().external_id,
            transaction.values().correlation_id,
            recorded_at,
            transaction.values().effective as chrono::NaiveDate,
        )
        .execute(op.as_executor())
        .await?;
        self.persist_events(op, transaction.events_mut()).await?;
        Ok(())
    }
}
