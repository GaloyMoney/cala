use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
use crate::entity::*;

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
        let n_new_events = events.persist(tx).await?;
        let journal = Journal::try_from(events)?;
        Ok(EntityUpdate {
            entity: journal,
            n_new_events,
        })
    }
}
