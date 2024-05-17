#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::{cursor::*, entity::*, error::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, query::*};

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
        tx: &mut Transaction<'_, Postgres>,
        new_account: NewAccount,
    ) -> Result<EntityUpdate<Account>, AccountError> {
        let id = new_account.id;
        sqlx::query!(
            r#"INSERT INTO cala_accounts (id, code, name, external_id)
            VALUES ($1, $2, $3, $4)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account.external_id,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_account.initial_events();
        let n_new_events = events.persist(tx).await?;
        let account = Account::try_from(events)?;
        Ok(EntityUpdate {
            entity: account,
            n_new_events,
        })
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
            WHERE a.external_id = $1"#,
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
        tx: &mut Transaction<'_, Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account: &mut Account,
    ) -> Result<(), AccountError> {
        sqlx::query!(
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            recorded_at
        )
        .execute(&mut **tx)
        .await?;
        account.events.persisted_at(tx, origin, recorded_at).await?;
        Ok(())
    }
}
