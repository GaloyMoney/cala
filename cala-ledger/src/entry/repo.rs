use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use tracing::instrument;

use super::{entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    entity::EntityEvents,
    primitives::{AccountId, EntryId, JournalId},
};

#[derive(Debug, Clone)]
pub(crate) struct EntryRepo {
    _pool: PgPool,
}

impl EntryRepo {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "trace",
        name = "cala_ledger.entries.create_all",
        skip(self, tx)
    )]
    pub(crate) async fn create_all(
        &self,
        entries: Vec<NewEntry>,
        tx: &mut Transaction<'_, Postgres>,
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
        query.execute(&mut **tx).await?;
        EntityEvents::batch_persist(tx, entries.into_iter().map(|n| n.initial_events())).await?;
        Ok(entry_values)
    }

    #[cfg(feature = "import")]
    pub(super) async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        entry: &mut Entry,
    ) -> Result<(), EntryError> {
        sqlx::query!(
            r#"INSERT INTO cala_entries (data_source_id, id, journal_id, account_id)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
            entry.values().journal_id as JournalId,
            entry.values().account_id as AccountId,
        )
        .execute(&mut **tx)
        .await?;
        entry.events.persist(tx, origin).await?;
        Ok(())
    }
}
