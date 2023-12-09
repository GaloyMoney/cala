use sqlx::{PgPool, Postgres, Transaction};
use tracing::instrument;

use super::{entity::*, error::*};
use crate::{entity::*, primitives::*};

/// Repository for working with `Account` entities.
#[derive(Debug, Clone)]
pub struct Accounts {
    pool: PgPool,
}
impl Accounts {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(
        &self,
        new_account: NewAccount,
    ) -> Result<EntityUpdate<AccountEvent>, AccountError> {
        let mut tx = self.pool.begin().await?;
        let res = self.create_in_tx(&mut tx, new_account).await?;
        tx.commit().await?;
        Ok(res)
    }

    #[instrument(name = "cala_ledger.accounts.create", skip(self, tx))]
    pub async fn create_in_tx<'a>(
        &self,
        tx: &mut Transaction<'a, Postgres>,
        new_account: NewAccount,
    ) -> Result<EntityUpdate<AccountEvent>, AccountError> {
        let id = new_account.id;
        sqlx::query!(
            r#"INSERT INTO cala_accounts (id, code, name, external_id)
            VALUES ($1, $2, $3, $4)"#,
            id as AccountId,
            new_account.code,
            new_account.name,
            new_account
                .external_id
                .clone()
                .unwrap_or_else(|| id.to_string()),
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_account.initial_events();
        Ok(events.persist(tx).await?)
    }
}
