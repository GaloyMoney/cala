use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
use crate::entity::*;

#[derive(Debug, Clone)]
pub(super) struct AccountRepo {
    _pool: PgPool,
}
impl AccountRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
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
}
