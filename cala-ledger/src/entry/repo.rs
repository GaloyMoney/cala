use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;

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
            r#"INSERT INTO cala_entries (data_source_id, id)
            VALUES ($1, $2)"#,
            origin as DataSourceId,
            entry.values().id as EntryId,
        )
        .execute(&mut **tx)
        .await?;
        entry.events.persist(tx, origin).await?;
        Ok(())
    }
}
