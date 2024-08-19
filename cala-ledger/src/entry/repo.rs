#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::*,
    primitives::{AccountId, EntryId, JournalId},
    query::{PaginatedQueryArgs, PaginatedQueryRet},
};

use super::{entity::*, error::*, EntryByCreatedAtCursor};

#[derive(Debug, Clone)]
pub(crate) struct EntryRepo {
    pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.entries.create_all",
        skip(self, db)
    )]
    pub(crate) async fn create_all(
        &self,
        db: &mut Transaction<'_, Postgres>,
        entries: Vec<NewEntry>,
    ) -> Result<Vec<EntryValues>, EntryError> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"INSERT INTO cala_entries
               (id, journal_id, account_id, transaction_id)"#,
        );
        let mut entry_values = Vec::new();
        query_builder.push_values(entries.iter(), |mut builder, entry: &NewEntry| {
            entry_values.push(entry.to_values());
            builder.push_bind(entry.id);
            builder.push_bind(entry.journal_id);
            builder.push_bind(entry.account_id);
            builder.push_bind(entry.transaction_id);
        });
        let query = query_builder.build();
        query.execute(&mut **db).await?;
        EntityEvents::batch_persist(db, entries.into_iter().map(|n| n.initial_events())).await?;
        Ok(entry_values)
    }

    pub(crate) async fn list_for_account(
        &self,
        account_id: AccountId,
        args: PaginatedQueryArgs<EntryByCreatedAtCursor>,
    ) -> Result<PaginatedQueryRet<Entry, EntryByCreatedAtCursor>, EntryError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
             WITH entries AS (
                SELECT id, created_at
                FROM cala_entries
                WHERE (created_at, id) < ($1, $2) OR ($1 IS NULL AND $2 IS NULL)
                AND account_id = $4
                ORDER BY created_at DESC, id
                LIMIT $3
              )
             SELECT a.id, e.sequence, e.event,
             a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
             FROM entries a
             JOIN cala_entry_events e ON a.id = e.id
             ORDER BY a.created_at DESC, a.id, e.sequence
            "#,
            args.after.as_ref().map(|c| c.created_at),
            args.after.as_ref().map(|c| c.id) as Option<EntryId>,
            args.first as i64 + 1,
            account_id as AccountId,
        )
        .fetch_all(&self.pool)
        .await?;

        let (entities, has_next_page) = EntityEvents::load_n::<Entry>(rows, args.first)?;

        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(EntryByCreatedAtCursor {
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
    pub(super) async fn import(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        entry: &mut Entry,
    ) -> Result<(), EntryError> {
        sqlx::query!(
            r#"INSERT INTO cala_entries (data_source_id, id, journal_id, account_id, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
            entry.values().journal_id as JournalId,
            entry.values().account_id as AccountId,
            recorded_at,
        )
        .execute(&mut **db)
        .await?;
        entry.events.persisted_at(db, origin, recorded_at).await?;
        Ok(())
    }
}
