#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::entity::EntityUpdate;
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct AccountSetRepo {
    _pool: PgPool,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_account_set: NewAccountSet,
    ) -> Result<EntityUpdate<AccountSet>, AccountSetError> {
        let id = new_account_set.id;
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (id, name)
            VALUES ($1, $2)"#,
            id as AccountSetId,
            new_account_set.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_account_set.initial_events();
        let n_new_events = events.persist(db).await?;
        let account_set = AccountSet::try_from(events)?;
        Ok(EntityUpdate {
            entity: account_set,
            n_new_events,
        })
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (data_source_id, id, name, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            account_set.values().id as AccountSetId,
            account_set.values().name,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        account_set
            .events
            .persisted_at(db, origin, recorded_at)
            .await?;
        Ok(())
    }
}
