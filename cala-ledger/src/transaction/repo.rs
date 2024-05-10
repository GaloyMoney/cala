use sqlx::{PgPool, Postgres};

use cala_types::primitives::*;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{DataSource, TransactionId, TxTemplateId},
};

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct TransactionRepo {
    _pool: PgPool,
}

impl TransactionRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
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
            .map_or_else(|| tx_id.to_string(), |id| id.to_string());
        sqlx::query!(
            r#"INSERT INTO cala_transactions (id, journal_id, tx_template_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4, $5)"#,
            tx_id as TransactionId,
            new_transaction.journal_id as JournalId,
            new_transaction.tx_template_id as TxTemplateId,
            correlation_id,
            new_transaction.external_id
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

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        origin: DataSourceId,
        transaction: &mut Transaction,
    ) -> Result<(), TransactionError> {
        sqlx::query!(
            r#"INSERT INTO cala_transactions (data_source_id, id, journal_id, tx_template_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
            transaction.values().id as TransactionId,
            transaction.values().journal_id as JournalId,
            transaction.values().tx_template_id as TxTemplateId,
            transaction.values().correlation_id,
            transaction.values().external_id,
        )
        .execute(&mut **tx)
        .await?;
        transaction.events.persist(tx, origin).await?;
        Ok(())
    }
}
