use sqlx::{PgPool, Postgres, Transaction};

use cala_types::query::*;

use super::{entity::*, error::*};
use crate::entity::*;

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
    ) -> Result<EntityUpdate<AccountEvent>, AccountError> {
        let id = new_account.id;
        sqlx::query!(
            r#"INSERT INTO cala_accounts (id, code, name, external_id, tags)
            VALUES ($1, $2, $3, $4, $5)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account.external_id,
            &new_account.tags
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_account.initial_events();
        Ok(events.persist(tx).await?)
    }

    pub async fn list(
        &self,
        query: PaginatedQueryArgs<AccountByNameCursor>,
    ) -> Result<PaginatedQueryRet<Account, AccountByNameCursor>, AccountError> {
        let rows = sqlx::query_as!(
            GenericEvent,
            r#"SELECT a.id, e.sequence, e.event
            FROM cala_accounts a
            JOIN cala_account_events e ON a.id = e.id
            WHERE ((a.name, a.id) > ($2, $1)) OR ($1 IS NULL AND $2 IS NULL)
            ORDER BY a.name, a.id, e.sequence
            LIMIT $3"#,
            query.after.as_ref().map(|c| c.id) as Option<AccountId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_all(&self.pool)
        .await?;
        let (nodes, has_next_page) = EntityEvents::load_n::<Account>(rows, query.first)?;
        let mut end_cursor = None;
        if let Some(last) = nodes.last() {
            end_cursor = Some(AccountByNameCursor {
                id: last.values.id,
                name: last.values.name.clone(),
            });
        }
        Ok(PaginatedQueryRet {
            nodes,
            has_next_page,
            end_cursor,
        })
    }
}
