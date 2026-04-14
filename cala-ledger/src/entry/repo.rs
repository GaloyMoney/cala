use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, AccountSetId, EntryId, JournalId, TransactionId},
};
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
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(crate) struct EntryRepo {
    #[allow(dead_code)]
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    #[instrument(
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

        let eventually_consistent: Option<bool> =
            sqlx::query_scalar("SELECT eventually_consistent FROM cala_accounts WHERE id = $1")
                .bind(account_set_id as AccountSetId)
                .fetch_optional(&self.pool)
                .await?;

        let Some(eventually_consistent) = eventually_consistent else {
            return Ok(es_entity::PaginatedQueryRet {
                entities: vec![],
                has_next_page: false,
                end_cursor: None,
            });
        };

        let (entities, has_next_page) = if eventually_consistent {
            let journal_id: JournalId =
                sqlx::query_scalar("SELECT journal_id FROM cala_account_sets WHERE id = $1")
                    .bind(account_set_id as AccountSetId)
                    .fetch_one(&self.pool)
                    .await?;
            match direction {
                es_entity::ListDirection::Ascending => {
                    es_entity::es_query!(
                        entity = Entry,
                        r#"
                            SELECT e.created_at, e.id
                            FROM cala_entries e
                            JOIN cala_account_set_member_accounts m
                              ON m.member_account_id = e.account_id
                            LEFT JOIN cala_account_sets s
                              ON s.id = m.member_account_id
                            WHERE m.account_set_id = $4
                              AND s.id IS NULL
                              AND e.journal_id = $5
                              AND (COALESCE((e.created_at, e.id) > ($3, $2), $2 IS NULL))
                            ORDER BY e.created_at ASC, e.id ASC
                            LIMIT $1"#,
                        (first + 1) as i64,
                        id as Option<EntryId>,
                        created_at as Option<chrono::DateTime<chrono::Utc>>,
                        account_set_id as AccountSetId,
                        journal_id as JournalId,
                    )
                    .fetch_n(executor, first)
                    .await?
                }
                es_entity::ListDirection::Descending => {
                    es_entity::es_query!(
                        entity = Entry,
                        r#"
                            SELECT e.created_at, e.id
                            FROM cala_entries e
                            JOIN cala_account_set_member_accounts m
                              ON m.member_account_id = e.account_id
                            LEFT JOIN cala_account_sets s
                              ON s.id = m.member_account_id
                            WHERE m.account_set_id = $4
                              AND s.id IS NULL
                              AND e.journal_id = $5
                              AND (COALESCE((e.created_at, e.id) < ($3, $2), $2 IS NULL))
                            ORDER BY e.created_at DESC, e.id DESC
                            LIMIT $1"#,
                        (first + 1) as i64,
                        id as Option<EntryId>,
                        created_at as Option<chrono::DateTime<chrono::Utc>>,
                        account_set_id as AccountSetId,
                        journal_id as JournalId,
                    )
                    .fetch_n(executor, first)
                    .await?
                }
            }
        } else {
            match direction {
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

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &Entry,
        new_events: es_entity::LastPersisted<'_, EntryEvent>,
    ) -> Result<(), sqlx::Error> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}
