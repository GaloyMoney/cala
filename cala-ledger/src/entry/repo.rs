use crate::primitives::{AccountId, AccountSetId, EntryId, JournalId, TransactionId};
use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use super::{entity::*, error::*};

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "Entry",
    columns(
        account_id(ty = "AccountId", list_for(by(created_at)), update(persist = false)),
        journal_id(ty = "JournalId", list_for(by(created_at)), update(persist = false)),
        transaction_id(
            ty = "TransactionId",
            list_for(by(created_at)),
            update(persist = false)
        ),
    ),
    tbl_prefix = "cala",
    persist_event_context = false
)]
/// Read side of the entry entity.
///
/// Entries are written exclusively by [`crate::posting`], which inserts the
/// rows and their events in the same statement as the transactions and
/// balances, and publishes `EntryCreated` itself — so this repo carries no
/// persist hook.
pub(crate) struct EntryRepo {
    pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[instrument(
        level = "debug",
        name = "entry.list_for_account_set_id_by_created_at",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_account_set_id_by_created_at(
        &self,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<entry_cursor::EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, entry_cursor::EntryByCreatedAtCursor>, EntryError>
    {
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
                            entity = Entry,
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
                            .fetch_n(executor, first)
                            .await?
                    },
                    es_entity::ListDirection::Descending => {
                        es_entity::es_query!(
                            entity = Entry,
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
                            .fetch_n(executor, first)
                            .await?
                    },
                };

        let end_cursor = entities
            .last()
            .map(entry_cursor::EntryByCreatedAtCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[instrument(
        level = "debug",
        name = "entry.list_for_journal_id_filtered_by_created_at",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn list_for_journal_id_filtered_by_created_at(
        &self,
        journal_id: JournalId,
        filter: EntriesFilter,
        query: es_entity::PaginatedQueryArgs<entry_cursor::EntryByCreatedAtCursor>,
        direction: es_entity::ListDirection,
    ) -> Result<es_entity::PaginatedQueryRet<Entry, entry_cursor::EntryByCreatedAtCursor>, EntryError>
    {
        let es_entity::PaginatedQueryArgs { first, after } = query;
        let (id, created_at) = if let Some(after) = after {
            (Some(after.id), Some(after.created_at))
        } else {
            (None, None)
        };
        let EntriesFilter {
            created_at_from,
            created_at_to,
            effective_from,
            effective_to,
        } = filter;

        let executor = &self.pool;

        // The outer query stays single-table on `cala_entries` (so `created_at` /
        // `id` are unambiguous and the cursor pagination matches the other
        // `list_for_*` methods); the effective-date range is applied as a
        // semi-join against `cala_transactions`. When neither effective bound is
        // set the disjunct short-circuits to the plain journal listing.
        let (entities, has_next_page) = match direction {
            es_entity::ListDirection::Ascending => {
                es_entity::es_query!(
                    entity = Entry,
                    r#"
                    SELECT created_at, id
                    FROM cala_entries
                    WHERE journal_id = $4
                      AND ($5::timestamptz IS NULL OR created_at >= $5::timestamptz)
                      AND ($6::timestamptz IS NULL OR created_at <= $6::timestamptz)
                      AND (
                        ($7::date IS NULL AND $8::date IS NULL)
                        OR transaction_id IN (
                          SELECT id FROM cala_transactions
                          WHERE ($7::date IS NULL OR effective >= $7::date)
                            AND ($8::date IS NULL OR effective <= $8::date)
                        )
                      )
                      AND (COALESCE((created_at, id) > ($3, $2), $2 IS NULL))
                    ORDER BY created_at ASC, id ASC
                    LIMIT $1"#,
                    (first + 1) as i64,
                    id as Option<EntryId>,
                    created_at as Option<chrono::DateTime<chrono::Utc>>,
                    journal_id as JournalId,
                    created_at_from as Option<chrono::DateTime<chrono::Utc>>,
                    created_at_to as Option<chrono::DateTime<chrono::Utc>>,
                    effective_from as Option<chrono::NaiveDate>,
                    effective_to as Option<chrono::NaiveDate>,
                )
                .fetch_n(executor, first)
                .await?
            }
            es_entity::ListDirection::Descending => {
                es_entity::es_query!(
                    entity = Entry,
                    r#"
                    SELECT created_at, id
                    FROM cala_entries
                    WHERE journal_id = $4
                      AND ($5::timestamptz IS NULL OR created_at >= $5::timestamptz)
                      AND ($6::timestamptz IS NULL OR created_at <= $6::timestamptz)
                      AND (
                        ($7::date IS NULL AND $8::date IS NULL)
                        OR transaction_id IN (
                          SELECT id FROM cala_transactions
                          WHERE ($7::date IS NULL OR effective >= $7::date)
                            AND ($8::date IS NULL OR effective <= $8::date)
                        )
                      )
                      AND (COALESCE((created_at, id) < ($3, $2), $2 IS NULL))
                    ORDER BY created_at DESC, id DESC
                    LIMIT $1"#,
                    (first + 1) as i64,
                    id as Option<EntryId>,
                    created_at as Option<chrono::DateTime<chrono::Utc>>,
                    journal_id as JournalId,
                    created_at_from as Option<chrono::DateTime<chrono::Utc>>,
                    created_at_to as Option<chrono::DateTime<chrono::Utc>>,
                    effective_from as Option<chrono::NaiveDate>,
                    effective_to as Option<chrono::NaiveDate>,
                )
                .fetch_n(executor, first)
                .await?
            }
        };

        let end_cursor = entities
            .last()
            .map(entry_cursor::EntryByCreatedAtCursor::from);

        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }
}
