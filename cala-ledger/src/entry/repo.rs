use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::primitives::{AccountId, EntryId, JournalId};

#[derive(Debug, Clone)]
pub(super) struct EntryRepo {
    _pool: PgPool,
}

impl EntryRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    #[cfg(feature = "import")]
    pub async fn import(
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
