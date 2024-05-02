use sqlx::{PgPool, Postgres, Transaction};

use cala_types::primitives::Tag;

use super::{cursor::*, entity::*, error::*};
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
            r#"INSERT INTO cala_accounts (id, code, name, external_id, tags)
            VALUES ($1, $2, $3, $4, $5)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account.external_id,
            &new_account.tags as &Vec<Tag>
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
}
