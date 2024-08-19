#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction as DbTransaction};

use std::collections::HashMap;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{JournalId, TransactionId, TxTemplateId},
    query::*,
};

use super::{entity::*, error::*, Transaction, TransactionByCreatedAtCursor};

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
            r#"INSERT INTO cala_transactions (id, journal_id, tx_template_id, correlation_id, external_id)
            VALUES ($1, $2, $3, $4, $5)"#,
            new_transaction.id as TransactionId,
            new_transaction.journal_id as JournalId,
            new_transaction.tx_template_id as TxTemplateId,
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

    pub async fn find_by_id(&self, id: TransactionId) -> Result<Transaction, TransactionError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_transactions a
            JOIN cala_transaction_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = $1
            ORDER BY e.sequence"#,
            id as TransactionId
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(transaction) => Ok(transaction),
            Err(EntityError::NoEntityEventsPresent) => Err(TransactionError::CouldNotFindById(id)),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn list(
        &self,
        args: PaginatedQueryArgs<TransactionByCreatedAtCursor>,
    ) -> Result<PaginatedQueryRet<Transaction, TransactionByCreatedAtCursor>, TransactionError>
    {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
        WITH transactions AS (
          SELECT id, created_at
          FROM cala_transactions
          WHERE (created_at, id) < ($1, $2) OR ($1 IS NULL AND $2 IS NULL)
          ORDER BY created_at DESC, id DESC
          LIMIT $3
        )
        SELECT t.id, e.sequence, e.event,
            t.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
        FROM transactions t
        JOIN cala_transaction_events e ON t.id = e.id
        ORDER BY t.created_at DESC, t.id DESC , e.sequence"#,
            args.after.as_ref().map(|c| c.created_at),
            args.after.as_ref().map(|c| c.id) as Option<TransactionId>,
            args.first as i64 + 1
        )
        .fetch_all(&self.pool)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<Transaction>(rows, args.first)?;

        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(TransactionByCreatedAtCursor {
                created_at: last.created_at(),
                id: last.values().id,
            });
        }

        Ok(PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
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
            r#"INSERT INTO cala_transactions (data_source_id, id, journal_id, tx_template_id, external_id, correlation_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            origin as DataSourceId,
            transaction.values().id as TransactionId,
            transaction.values().journal_id as JournalId,
            transaction.values().tx_template_id as TxTemplateId,
            transaction.values().external_id,
            transaction.values().correlation_id,
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
