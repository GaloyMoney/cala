#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, primitives::AccountId, primitives::JournalId};

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct AccountSetRepo {
    pool: PgPool,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_sets (id, journal_id, name)
            VALUES ($1, $2, $3)"#,
            new_account_set.id as AccountSetId,
            new_account_set.journal_id as JournalId,
            new_account_set.name,
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_account_set.initial_events();
        events.persist(db).await?;
        let account_set = AccountSet::try_from(events)?;
        Ok(account_set)
    }

    pub async fn add_member_account(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (account_set_id, account_id)
            VALUES ($1, $2)"#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_account_sets a
            JOIN cala_account_set_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = $1"#,
            account_set_id as AccountSetId
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account_set) => Ok(account_set),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(AccountSetError::CouldNotFindById(account_set_id))
            }
            Err(e) => Err(e.into()),
        }
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
            r#"INSERT INTO cala_account_sets (data_source_id, id, journal_id, name, created_at)
            VALUES ($1, $2, $3, $4, $5)"#,
            origin as DataSourceId,
            account_set.values().id as AccountSetId,
            account_set.values().journal_id as JournalId,
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

    pub async fn import_member_account(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (data_source_id, account_set_id, account_id, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            account_id as AccountId,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }
}
