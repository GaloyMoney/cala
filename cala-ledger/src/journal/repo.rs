use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, primitives::DataSource};

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
            r#"INSERT INTO cala_journals (id, name, external_id)
            VALUES ($1, $2, $3)"#,
            id as JournalId,
            new_journal.name,
            new_journal.external_id,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_journal.initial_events();
        let n_new_events = events.persist(tx, DataSource::Local).await?;
        let journal = Journal::try_from(events)?;
        Ok(EntityUpdate {
            entity: journal,
            n_new_events,
        })
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        journal: &mut Journal,
    ) -> Result<(), JournalError> {
        sqlx::query!(
            r#"INSERT INTO cala_journals (data_source_id, id, name, external_id)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            journal.values().id as JournalId,
            journal.values().name,
            journal.values().external_id,
        )
        .execute(&mut **tx)
        .await?;
        journal.events.persist(tx, origin).await?;
        Ok(())
    }
}
