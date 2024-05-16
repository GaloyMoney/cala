pub mod error;
mod repo;

use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    entity::EntityUpdate,
    outbox::*,
    primitives::{DataSource, JournalId},
};
use cala_types::entry::EntryValues;

use error::BalanceError;
use repo::*;

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    outbox: Outbox,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            outbox,
            _pool: pool.clone(),
        }
    }

    pub(crate) async fn update_balances(
        &self,
        tx: Transaction<'_, Postgres>,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
    ) -> Result<(), BalanceError> {
        // let balances = self.repo.update_balances(tx, entries)?;
        // let events = balances
        //     .iter()
        //     .map(|values| OutboxEventPayload::BalanceUpdated {
        //         source: DataSource::Local,
        //         balance: values.clone(),
        //     })
        //     .collect();
        Ok(())
    }
}
