#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};

use std::collections::HashMap;

use super::{cursor::*, entity::*};
#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, errors::*, primitives::DebitOrCredit, query::*};

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
    ) -> Result<EntityUpdate<Account>, OneOf<(ConstraintVioliation, UnexpectedDbError)>> {
        let id = new_account.id;
        sqlx::query!(
            r#"INSERT INTO cala_accounts (id, code, name, external_id, normal_balance_type)
            VALUES ($1, $2, $3, $4, $5)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account.external_id,
            new_account.normal_balance_type as DebitOrCredit,
        )
        .execute(&mut **tx)
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let mut events = new_account.initial_events();
        let n_new_events = events.persist(tx).await.map_err(OneOf::broaden)?;
        let account = Account::try_from(events).expect("Couldn't hydrate new entity");
        Ok(EntityUpdate {
            entity: account,
            n_new_events,
        })
    }

    pub async fn find(
        &self,
        account_id: AccountId,
    ) -> Result<Account, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id = $1"#,
            account_id as AccountId
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        EntityEvents::load_first(rows).map_err(OneOf::broaden)
    }

    pub(super) async fn find_all(
        &self,
        ids: &[AccountId],
    ) -> Result<HashMap<AccountId, AccountValues>, OneOf<(HydratingEntityError, UnexpectedDbError)>>
    {
        let mut query_builder = QueryBuilder::new(
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.id IN"#,
        );
        query_builder.push_tuples(ids, |mut builder, account_id| {
            builder.push_bind(account_id);
        });
        query_builder.push(r#"ORDER BY a.id, e.sequence"#);
        let query = query_builder.build_query_as::<GenericEvent>();
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let n = rows.len();
        let ret = EntityEvents::load_n(rows, n)
            .map_err(OneOf::broaden)?
            .0
            .into_iter()
            .map(|account: Account| (account.values().id, account.into_values()))
            .collect();
        Ok(ret)
    }

    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<Account, OneOf<(EntityNotFound, HydratingEntityError, UnexpectedDbError)>> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event,
                a.created_at AS entity_created_at, e.recorded_at AS event_recorded_at
            FROM cala_accounts a
            JOIN cala_account_events e
            ON a.data_source_id = e.data_source_id
            AND a.id = e.id
            WHERE a.data_source_id = '00000000-0000-0000-0000-000000000000'
            AND a.external_id = $1"#,
            external_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        EntityEvents::load_first(rows).map_err(OneOf::broaden)
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<
        PaginatedQueryRet<Account, AccountByNameCursor>,
        OneOf<(HydratingEntityError, UnexpectedDbError)>,
    > {
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
        .await
        .map_err(|e| OneOf::new(UnexpectedDbError(e)))?;
        let (entities, has_next_page) =
            EntityEvents::load_n::<Account>(rows, query.first).map_err(OneOf::broaden)?;
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
    ) -> Result<(), OneOf<(UnexpectedDbError,)>> {
        sqlx::query!(
            r#"INSERT INTO cala_accounts (data_source_id, id, code, name, external_id, normal_balance_type, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            origin as DataSourceId,
            account.values().id as AccountId,
            account.values().code,
            account.values().name,
            account.values().external_id,
            account.values().normal_balance_type as DebitOrCredit,
            recorded_at
        )
        .execute(&mut **tx)
        .await.map_err(UnexpectedDbError)?;
        account.events.persisted_at(tx, origin, recorded_at).await?;
        Ok(())
    }
}
