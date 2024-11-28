#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

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
        db: &mut Transaction<'_, Postgres>,
        new_journal: NewJournal,
    ) -> Result<Journal, JournalError> {
        unimplemented!()
        // let id = new_journal.id;
        // sqlx::query!(
        //     r#"INSERT INTO cala_journals (id, name)
        //     VALUES ($1, $2)"#,
        //     id as JournalId,
        //     new_journal.name,
        // )
        // .execute(&mut **db)
        // .await?;
        // let mut events = new_journal.initial_events();
        // events.persist(db).await?;
        // let journal = Journal::try_from(events)?;
        // Ok(journal)
    }
    pub async fn persist_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        unimplemented!()
        // sqlx::query!(
        //     r#"UPDATE cala_journals
        //     SET name = $2
        //     WHERE id = $1 AND data_source_id = '00000000-0000-0000-0000-000000000000'"#,
        //     journal.values().id as JournalId,
        //     journal.values().name,
        // )
        // .execute(&mut **db)
        // .await?;
        // journal.events.persist(db).await?;
        // Ok(())
    }

    pub(super) async fn find_all<T: From<Journal>>(
        &self,
        ids: &[JournalId],
    ) -> Result<HashMap<JournalId, T>, JournalError> {
        unimplemented!()
        // let rows = sqlx::query_as!(
        //     GenericEvent,
        //     r#"SELECT j.id, e.sequence, e.event,
        //         j.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
        //     FROM cala_journals j
        //     JOIN cala_journal_events e
        //     ON j.data_source_id = e.data_source_id
        //     AND j.id = e.id
        //     WHERE j.data_source_id = '00000000-0000-0000-0000-000000000000'
        //     AND j.id = ANY($1)
        //     ORDER BY j.id, e.sequence"#,
        //     ids as &[JournalId]
        // )
        // .fetch_all(&self.pool)
        // .await?;
        // let n = rows.len();
        // let ret = EntityEvents::load_n(rows, n)?
        //     .0
        //     .into_iter()
        //     .map(|journal: Journal| (journal.values().id, T::from(journal)))
        //     .collect();
        // Ok(ret)
    }

    pub async fn find(&self, id: JournalId) -> Result<Journal, JournalError> {
        unimplemented!()
        // let rows = sqlx::query_as!(
        //     GenericEvent,
        //     r#"SELECT j.id, e.sequence, e.event,
        //         j.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
        //     FROM cala_journals j
        //     JOIN cala_journal_events e
        //     ON j.data_source_id = e.data_source_id
        //     AND j.id = e.id
        //     WHERE j.data_source_id = '00000000-0000-0000-0000-000000000000'
        //     AND j.id = $1
        //     ORDER BY e.sequence"#,
        //     id as JournalId
        // )
        // .fetch_all(&self.pool)
        // .await?;
        // match EntityEvents::load_first(rows) {
        //     Ok(account) => Ok(account),
        //     Err(EntityError::NoEntityEventsPresent) => Err(JournalError::CouldNotFindById(id)),
        //     Err(e) => Err(e.into()),
        // }
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        unimplemented!()
        // sqlx::query!(
        //     r#"INSERT INTO cala_journals (data_source_id, id, name, created_at)
        //     VALUES ($1, $2, $3, $4)"#,
        //     origin as DataSourceId,
        //     journal.values().id as JournalId,
        //     journal.values().name,
        //     recorded_at
        // )
        // .execute(&mut **db)
        // .await?;
        // journal.events.persisted_at(db, origin, recorded_at).await?;
        // Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn find_imported(
        &self,
        journal_id: JournalId,
        origin: DataSourceId,
    ) -> Result<Journal, JournalError> {
        unimplemented!()
        // let rows = sqlx::query_as!(
        //     GenericEvent,
        //     r#"SELECT a.id, e.sequence, e.event,
        //         a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
        //     FROM cala_journals a
        //     JOIN cala_journal_events e
        //     ON a.data_source_id = e.data_source_id
        //     AND a.id = e.id
        //     WHERE a.data_source_id = $1
        //     AND a.id = $2
        //     ORDER BY e.sequence"#,
        //     origin as DataSourceId,
        //     journal_id as JournalId
        // )
        // .fetch_all(&self.pool)
        // .await?;
        // match EntityEvents::load_first(rows) {
        //     Ok(journal) => Ok(journal),
        //     Err(EntityError::NoEntityEventsPresent) => {
        //         Err(JournalError::CouldNotFindById(journal_id))
        //     }
        //     Err(e) => Err(e.into()),
        // }
    }

    #[cfg(feature = "import")]
    pub async fn persist_at_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        unimplemented!()
        // sqlx::query!(
        //     r#"UPDATE cala_journals
        //     SET name = $3
        //     WHERE data_source_id = $1 AND id = $2"#,
        //     origin as DataSourceId,
        //     journal.values().id as JournalId,
        //     journal.values().name,
        // )
        // .execute(&mut **db)
        // .await?;
        // journal.events.persisted_at(db, origin, recorded_at).await?;
        // Ok(())
    }
}
