#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use std::collections::HashMap;

use super::{cursor::*, entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, primitives::DebitOrCredit, query::*};

#[derive(Debug, Clone)]
pub(super) struct AccountRepo {
    pool: PgPool,
}

impl AccountRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        new_account: NewAccount,
    ) -> Result<Account, AccountError> {
        let id = new_account.id;
        sqlx::query!(
            r#"INSERT INTO cala_accounts (id, code, name, external_id, normal_balance_type, eventually_consistent)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account.external_id,
            new_account.normal_balance_type as DebitOrCredit,
            new_account.eventually_consistent
        )
        .execute(&mut **db)
        .await?;
        let mut events = new_account.initial_events();
        events.persist(db).await?;
        let account = Account::try_from(events)?;
        Ok(account)
    }

    pub async fn persist_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        sqlx::query!(
            r#"UPDATE cala_accounts
            SET code = $2, name = $3, external_id = $4, normal_balance_type = $5
            WHERE id = $1 AND data_source_id = '00000000-0000-0000-0000-000000000000'"#,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
        )
        .execute(&mut **db)
        .await?;
        account.events.persist(db).await?;
        Ok(())
    }

    pub async fn find(&self, account_id: AccountId) -> Result<Account, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = $1
            ORDER BY e.sequence"#,
            account_id as AccountId
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account) => Ok(account),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(AccountError::CouldNotFindById(account_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub(super) async fn find_all<T: From<Account>>(
        &self,
        ids: &[AccountId],
    ) -> Result<HashMap<AccountId, T>, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = ANY($1)
            ORDER BY a.id, e.sequence"#,
            ids as &[AccountId]
        )
        .fetch_all(&self.pool)
        .await?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)?
            .0
            .into_iter()
            .map(|account: Account| (account.values().id, T::from(account)))
            .collect();
        Ok(ret)
    }

    pub async fn find_by_external_id(&self, external_id: String) -> Result<Account, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.external_id = $1
            ORDER BY e.sequence"#,
            external_id
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account) => Ok(account),
            Err(EntityError::NoEntityEventsPresent) => {
                Err(AccountError::CouldNotFindByExternalId(external_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn find_by_code(&self, code: String) -> Result<Account, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.code = $1
            ORDER BY e.sequence"#,
            code
        )
        .fetch_all(&self.pool)
        .await?;
        match EntityEvents::load_first(rows) {
            Ok(account) => Ok(account),
            Err(EntityError::NoEntityEventsPresent) => Err(AccountError::CouldNotFindByCode(code)),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<PaginatedQueryRet<Account, AccountByNameCursor>, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"
            WITH accounts AS (
              SELECT id, name, created_at
              FROM cala_accounts
              WHERE ((name, id) > ($2, $1)) OR ($1 IS NULL AND $2 IS NULL)
              ORDER BY name, id
              LIMIT $3
            )
            SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM accounts a
            JOIN cala_account_events e ON a.id = e.id
            ORDER BY a.name, a.id, e.sequence"#,
            query.after.as_ref().map(|c| c.id) as Option<AccountId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(&self.pool)
        .await?;
        let (entities, has_next_page) = EntityEvents::load_n::<Account>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        db: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        sqlx::query!(
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, normal_balance_type, eventually_consistent, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
            account.values().config.eventually_consistent,
            recorded_at
        )
        .execute(&mut **db)
        .await?;
        account.events.persisted_at(db, origin, recorded_at).await?;
        Ok(())
    }
}
