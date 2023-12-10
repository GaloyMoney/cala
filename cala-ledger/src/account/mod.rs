//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use tracing::instrument;

use crate::{outbox::*, primitives::*};

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
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: AccountRepo::new(pool),
            outbox: Outbox::new(pool),
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
