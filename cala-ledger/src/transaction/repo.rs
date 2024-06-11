#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction as DbTransaction};

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{JournalId, TransactionId},
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
        db: &mut DbTransaction<'_, Postgres>,
        new_transaction: NewTransaction,
    ) -> Result<Transaction, TransactionError> {
        sqlx::query!(
            r#"INSERT INTO cala_transactions (id, journal_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4)"#,
            new_transaction.id as TransactionId,
            new_transaction.journal_id as JournalId,
            new_transaction.correlation_id,
            new_transaction.external_id
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_transaction.initial_events();
        events.persist(db).await?;
        let transaction = Transaction::try_from(events)?;
        Ok(transaction)
    }

    pub(super) async fn find_all<T: From<Transaction>>(
        &self,
        ids: &[TransactionId],
    ) -> Result<HashMap<TransactionId, T>, TransactionError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT t.id, e.sequence, e.event,
                t.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_transactions t
            JOIN cala_transaction_events e
            ON t.data_source_id = e.data_source_id
            AND t.id = e.id
            WHERE t.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND t.id = ANY($1)
            ORDER BY t.id, e.sequence"#,
            ids as &[TransactionId]
        )
        .fetch_all(&self.pool)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|transaction: Transaction| (transaction.values().id, T::from(transaction)))
            .collect();
        Ok(ret)
    }

    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Transaction, TransactionError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_transactions a
            JOIN cala_transaction_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.external_id = $1
            ORDER BY e.sequence"#,
            external_id
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(transaction) => Ok(transaction),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(TransactionError::CouldNotFindByExternalId(external_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        db: &mut DbTransaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        transaction: &mut Transaction,
    ) -> Result<(), TransactionError> {
        sqlx::query!(
            r#"INSERT INTO cala_transactions (data_source_id, id, journal_id, external_id, correlation_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
            transaction.values().id as TransactionId,
            transaction.values().journal_id as JournalId,
            transaction.values().correlation_id,
            transaction.values().external_id,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        transaction
            .events
            .persisted_at(db, origin, recorded_at)
            .await?;
        Ok(())
    }
}
