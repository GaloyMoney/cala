use sqlx::{PgPool, Postgres};

use cala_types::primitives::*;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{CorrelationId, DataSource, TransactionId, TxTemplateId},
};

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct TransactionRepo {
    pool: PgPool,
}

impl TransactionRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        new_transaction: NewTransaction,
    ) -> Result<EntityUpdate<Transaction>, TransactionError> {
        let tx_id = new_transaction.id;
        let correlation_id = new_transaction
            .correlation_id
            .as_ref()
            .map_or_else(|| uuid::Uuid::from(tx_id), |c_id| c_id.into());
        let external_id = new_transaction
            .external_id
            .as_ref()
            .map_or_else(|| tx_id.to_string(), |e_id| e_id.to_string());
        sqlx::query!(
            r#"INSERT INTO cala_transactions (id, journal_id, tx_template_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4, $5)"#,
            tx_id as TransactionId,
            new_transaction.journal_id as JournalId,
            new_transaction.tx_template_id as TxTemplateId,
            correlation_id,
            external_id,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_transaction.initial_events();
        let n_new_events = events.persist(tx, DataSource::Local).await?;
        let transaction = Transaction::try_from(events)?;
        Ok(EntityUpdate {
            entity: transaction,
            n_new_events,
        })
    }

    pub async fn list_by_template_id(
        &self,
        tx_template_id: TxTemplateId,
    ) -> Result<Vec<Transaction>, TransactionError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            SELECT t.id, e.sequence, e.event, t.created_at as entity_created_at, e.recorded_at as event_recorded_at
            FROM cala_transactions t
            JOIN cala_transaction_events e ON t.id = e.id
            WHERE t.tx_template_id = $1
            ORDER BY t.id, e.sequence
            "#,
            tx_template_id as TxTemplateId
        ).fetch_all(&self.pool).await?;
        let n = rows.len();
        let (transactions, ..) = EntityEvents::load_n::<Transaction>(rows, n)?;

        Ok(transactions)
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        origin: DataSourceId,
        mut transaction: Transaction,
    ) -> Result<(), TransactionError> {
        sqlx::query!(
            r#"INSERT INTO cala_transactions (data_source_id, id, journal_id, tx_template_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
            transaction.values().id as TransactionId,
            transaction.values().journal_id as JournalId,
            transaction.values().tx_template_id as TxTemplateId,
            transaction.values().correlation_id as CorrelationId,
            transaction.values().external_id,
        )
        .execute(&mut **tx)
        .await?;
        transaction.events.persist(tx, origin).await?;
        Ok(())
    }
}
