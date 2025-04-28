use crate::primitives::{AccountId, AccountSetId, DataSourceId, EntryId, JournalId, TransactionId};
use es_entity::*;
use sqlx::PgPool;

use super::{entity::*, error::*};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Entry",
    err = "EntryError",
    columns(
        account_id(ty = "AccountId", list_for, update(persist = false)),
        journal_id(ty = "JournalId", list_for, update(persist = false)),
        transaction_id(ty = "TransactionId", list_for, update(persist = false)),
        data_source_id(
            ty = "DataSourceId",
            create(accessor = "data_source().into()"),
            update(persist = false),
        ),
    ),
    tbl_prefix = "cala"
)]
pub(crate) struct EntryRepo {
    #[allow(dead_code)]
    pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub(super) async fn list_for_account_set_id_by_created_at(
        &self,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<entry_cursor::EntriesByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<
        es_entity::PaginatedQueryRet<Entry, entry_cursor::EntriesByCreatedAtCursor>,
        EntryError,
    > {
        let es_entity::PaginatedQueryArgs { first, after } = query;
        let (id, created_at) = if let Some(after) = after {
            (Some(after.id), Some(after.created_at))
        } else {
            (None, None)
        };

        let executor = &self.pool;

        let (entities, has_next_page) = match direction {
                    es_entity::ListDirection::Ascending => {
                        es_entity::es_query!(
                            "cala",
                            executor,
                            r#"
                            SELECT created_at, id
                            FROM cala_entries
                            JOIN cala_balance_history ON cala_entries.id = cala_balance_history.latest_entry_id
                            WHERE cala_balance_history.account_id = $4
                              AND (COALESCE((created_at, id) > ($3, $2), $2 IS NULL))
                            ORDER BY created_at ASC, id ASC
                            LIMIT $1"#,
                            (first + 1) as i64,
                            id as Option<EntryId>,
                            created_at as Option<chrono::DateTime<chrono::Utc>>,
                            account_set_id as AccountSetId,
                        )
                            .fetch_n(first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            "cala",
                            executor,
                            r#"
                            SELECT created_at, id
                            FROM cala_entries
                            JOIN cala_balance_history ON cala_entries.id = cala_balance_history.latest_entry_id
                            WHERE cala_balance_history.account_id = $4
                              AND (COALESCE((created_at, id) < ($3, $2), $2 IS NULL))
                            ORDER BY created_at DESC, id DESC
                            LIMIT $1"#,
                            (first + 1) as i64,
                            id as Option<EntryId>,
                            created_at as Option<chrono::DateTime<chrono::Utc>>,
                            account_set_id as AccountSetId,
                        )
                            .fetch_n(first)
                            .await?
                    },
                };

        let end_cursor = entities
            .last()
            .map(entry_cursor::EntriesByCreatedAtCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[cfg(feature = "import")]
    pub(super) async fn import(
        &self,
        op: &mut DbOp<'_>,
        origin: DataSourceId,
        entry: &mut Entry,
    ) -> Result<(), EntryError> {
        let recorded_at = op.now();
        sqlx::query!(
            r#"INSERT INTO cala_entries (data_source_id, id, journal_id, account_id, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
            entry.values().journal_id as JournalId,
            entry.values().account_id as AccountId,
            recorded_at,
        )
        .execute(&mut **op.tx())
        .await?;
        self.persist_events(op, entry.events_mut()).await?;
        Ok(())
    }
}
