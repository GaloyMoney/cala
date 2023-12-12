//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use cala_types::query::PaginatedQueryArgs;
use sqlx::PgPool;
use tracing::instrument;

use crate::outbox::*;

pub use entity::*;
use error::*;
use repo::*;

/// Service for working with `Account` entities.
#[derive(Clone)]
pub struct Accounts {
    repo: AccountRepo,
    outbox: Outbox,
    pool: PgPool,
}

impl Accounts {
    pub fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: AccountRepo::new(pool),
            outbox,
            pool: pool.clone(),
        }
    }

    #[instrument(name = "cala_ledger.accounts.create", skip(self))]
    pub async fn create(&self, new_account: NewAccount) -> Result<AccountId, AccountError> {
        let mut tx = self.pool.begin().await?;
        let res = self.repo.create_in_tx(&mut tx, new_account).await?;
        self.outbox.persist_events(tx, res.new_events).await?;
        Ok(res.id)
    }

    #[instrument(name = "cala_ledger.accounts.list_paginated", skip(self))]
    pub async fn list_paginated(
        &self,
        query: PaginatedQueryArgs<AccountId>,
    ) -> Result<(), AccountError> {
        unimplemented!()
        // let res = self
        //     .repo
        //     .list_via_curser(&mut tx, after, before, first, last)
        //     .await?;
        // Ok(res.connection)
    }
}

impl From<AccountEvent> for OutboxEventPayload {
    fn from(event: AccountEvent) -> Self {
        match event {
            AccountEvent::Initialized { values: account } => {
                OutboxEventPayload::AccountCreated { account }
            }
        }
    }
}
