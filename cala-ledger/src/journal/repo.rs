#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};

use std::collections::HashMap;

use super::{entity::*, error::*};
use crate::entity::*;
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;

#[derive(Debug, Clone)]
pub(super) struct JournalRepo {
    pool: PgPool,
}

impl JournalRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        new_journal: NewJournal,
    ) -> Result<EntityUpdate<Journal>, JournalError> {
        let id = new_journal.id;
        sqlx::query!(
            r#"INSERT INTO cala_journals (id, name)
            VALUES ($1, $2)"#,
            id as JournalId,
            new_journal.name,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_journal.initial_events();
        let n_new_events = events.persist(tx).await?;
        let journal = Journal::try_from(events)?;
        Ok(EntityUpdate {
            entity: journal,
            n_new_events,
        })
    }

    pub(super) async fn find_all(
        &self,
        ids: &[JournalId],
    ) -> Result<HashMap<JournalId, JournalValues>, JournalError> {
        let mut query_builder = QueryBuilder::new(
            r#"SELECT j.id, e.sequence, e.event,
                j.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_journals j
            JOIN cala_journal_events e
            ON j.data_source_id = e.data_source_id
            AND j.id = e.id
            WHERE j.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND j.id IN"#,
        );
        query_builder.push_tuples(ids, |mut builder, account_id| {
            builder.push_bind(account_id);
        });
        query_builder.push(r#"ORDER BY j.id, e.sequence"#);
        let query = query_builder.build_query_as::<GenericEvent>();
        let rows = query.fetch_all(&self.pool).await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|account: Journal| (account.values().id, account.into_values()))
            .collect();
        Ok(ret)
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        sqlx::query!(
            r#"INSERT INTO cala_journals (data_source_id, id, name, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            journal.values().id as JournalId,
            journal.values().name,
            recorded_at
        )
        .execute(&mut **tx)
        .await?;
        journal.events.persisted_at(tx, origin, recorded_at).await?;
        Ok(())
    }
}
