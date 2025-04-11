#[cfg(feature = "import")]
use es_entity::DbOp;

use es_entity::*;
use sqlx::PgPool;

use crate::primitives::*;

use super::{entity::*, error::TransactionError};
use transaction_cursor::TransactionsByCreatedAtCursor;

#[derive(EsRepo, Clone)]
#[es_repo(
    entity = "Transaction",
    err = "TransactionError",
    columns(
        external_id(ty = "Option<String>", update(persist = false)),
        correlation_id(ty = "String", update(persist = false)),
        journal_id(ty = "JournalId", update(persist = false)),
        tx_template_id(ty = "TxTemplateId", update(persist = false)),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala"
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
        op: &mut DbOp<'_>,
        origin: DataSourceId,
        transaction: &mut Transaction,
    ) -> Result<(), TransactionError> {
        let recorded_at = op.now();
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
        .execute(&mut **op.tx())
        .await?;
        self.persist_events(op, &mut transaction.events).await?;
        Ok(())
    }

    pub async fn list_for_template_ids_by_created_at(
        &self,
        template_ids: &[TxTemplateId],
        query: es_entity::PaginatedQueryArgs<TransactionsByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<
        es_entity::PaginatedQueryRet<Transaction, TransactionsByCreatedAtCursor>,
        TransactionError,
    > {
        let rows = match direction {
            ListDirection::Ascending => {
                sqlx::query_as!(
                    transaction_repo_types::Repo__DbEvent,
                    r#"
                      SELECT tx.id AS "entity_id: TransactionId", e.sequence, e.event, e.recorded_at
                      FROM cala_transactions tx
                      LEFT JOIN cala_transaction_events e ON tx.id = e.id
                      WHERE tx.tx_template_id = ANY($1) AND (tx.created_at > $2 OR $2 IS NULL)
                      ORDER BY tx.created_at ASC
                      LIMIT $3
                    "#,
                    template_ids as &[TxTemplateId],
                    query.after.as_ref().map(|c| c.created_at)
                        as Option<chrono::DateTime<chrono::Utc>>,
                    query.first as i64 + 1
                )
                .fetch_all(&self.pool)
                .await?
            }
            ListDirection::Descending => {
                sqlx::query_as!(
                    transaction_repo_types::Repo__DbEvent,
                    r#"
                      SELECT tx.id AS "entity_id: TransactionId", e.sequence, e.event, e.recorded_at
                      FROM cala_transactions tx
                      LEFT JOIN cala_transaction_events e ON tx.id = e.id
                      WHERE tx.tx_template_id = ANY($1) AND (tx.created_at > $2 OR $2 IS NULL)
                      ORDER BY tx.created_at DESC
                      LIMIT $3
                    "#,
                    template_ids as &[TxTemplateId],
                    query.after.as_ref().map(|c| c.created_at)
                        as Option<chrono::DateTime<chrono::Utc>>,
                    query.first as i64 + 1
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        let (entities, has_next_page) = EntityEvents::load_n::<Transaction>(rows, query.first)?;
        let end_cursor = entities.last().map(|last| TransactionsByCreatedAtCursor {
            id: last.values().id,
            created_at: last.values().created_at,
        });
        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }
}
