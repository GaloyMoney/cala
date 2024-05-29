#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use std::collections::HashMap;

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
    ) -> Result<DateTime<Utc>, AccountSetError> {
        let row = sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
            VALUES ($1, $2)
            RETURNING created_at"#,
            account_set_id as AccountSetId,
            account_id as AccountId,
        )
        .fetch_one(&mut **db)
        .await?;
        Ok(row.created_at)
    }

    pub async fn add_member_set(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<DateTime<Utc>, AccountSetError> {
        let row = sqlx::query!(
            r#"
            WITH member_account_insert AS (
                INSERT INTO cala_account_set_member_accounts (account_set_id, member_account_id)
                VALUES ($1, $2)
            )
            INSERT INTO cala_account_set_member_account_sets (account_set_id, member_account_set_id)
            VALUES ($1, $2)
            RETURNING created_at"#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .fetch_one(&mut **db)
        .await?;
        Ok(row.created_at)
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

    pub(super) async fn find_all<T: From<AccountSet>>(
        &self,
        ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT s.id, e.sequence, e.event,
                s.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts s
            JOIN cala_account_set_events e
            ON s.data_source_id = e.data_source_id
            AND s.id = e.id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND s.id = ANY($1)
            ORDER BY s.id, e.sequence"#,
            ids as &[AccountSetId]
        )
        .fetch_all(&self.pool)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|account: AccountSet| (account.values().id, T::from(account)))
            .collect();
        Ok(ret)
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

    #[cfg(feature = "import")]
    pub async fn import_member_account(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_accounts (data_source_id, account_set_id, member_account_id, created_at)
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

    #[cfg(feature = "import")]
    pub async fn import_member_set(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            r#"INSERT INTO cala_account_set_member_account_sets (data_source_id, account_set_id, member_account_set_id, created_at)
            VALUES ($1, $2, $3, $4)"#,
            origin as DataSourceId,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub async fn fetch_mappings(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
          WITH RECURSIVE account_sets_cte AS (
            -- Base case: Direct member accounts
            SELECT m.member_account_id AS account_id, m.account_set_id, s.data_source_id
            FROM cala_account_set_member_accounts m
            JOIN cala_account_sets s
            ON s.id = m.account_set_id AND s.data_source_id = m.data_source_id
            WHERE s.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND s.journal_id = $1
            AND m.member_account_id = ANY($2)

            UNION ALL
            -- Recursive case: Account sets that are members of other account sets
            SELECT c.account_id, mas.account_set_id, c.data_source_id
            FROM account_sets_cte c
            JOIN cala_account_set_member_account_sets mas
                ON c.account_set_id = mas.member_account_set_id
                AND mas.data_source_id = c.data_source_id
          )
          SELECT DISTINCT account_id AS "account_id!: AccountId", account_set_id AS "account_set_id!: AccountSetId"
          FROM account_sets_cte"#,
            journal_id as JournalId,
            account_ids as &[AccountId]
        )
        .fetch_all(&self.pool)
        .await?;
        let mut mappings = HashMap::new();
        for row in rows {
            let account_id = row.account_id;
            let account_set_id = row.account_set_id;
            mappings
                .entry(account_id)
                .or_insert_with(Vec::new)
                .push(account_set_id);
        }
        Ok(mappings)
    }
}
