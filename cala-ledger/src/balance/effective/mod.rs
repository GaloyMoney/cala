mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

// pub use cala_types::balance::{BalanceAmount, BalanceSnapshot};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{ledger_operation::*, primitives::JournalId};

use super::{account_balance::*, error::BalanceError};

use repo::*;

#[derive(Clone)]
pub struct EffectiveBalances {
    repo: EffectiveBalanceRepo,
    _pool: PgPool,
}
impl EffectiveBalances {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self {
            repo: EffectiveBalanceRepo::new(pool),
            _pool: pool.clone(),
        }
    }

    #[instrument(
        name = "cala_ledger.balance.effective.find_cumulative",
        skip(self),
        err
    )]
    pub async fn find_cumulative(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        unimplemented!()
    }

    #[instrument(name = "cala_ledger.balance.find_in_op", skip(self, _op), err)]
    pub async fn find_in_op(
        &self,
        _op: &mut LedgerOperation<'_>,
        _journal_id: JournalId,
        _account_id: impl Into<AccountId> + std::fmt::Debug,
        _currency: Currency,
        _date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        // self.repo
        //     .find_in_tx(op.tx(), journal_id, account_id.into(), currency)
        //     .await
        unimplemented!()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_cumulative_balances_in_tx(
        &self,
        db: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        journal_id: JournalId,
        _entries: Vec<EntryValues>,
        effective: NaiveDate,
        _created_at: DateTime<Utc>,
        _account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
        all_involved_balances: HashSet<(AccountId, Currency)>,
    ) -> Result<(), BalanceError> {
        self.repo
            .find_for_update(db, journal_id, all_involved_balances, effective)
            .await?;
        // let mut op = LedgerOperation::init(&self._pool, &self._outbox).await?;
        // self.update_balances(op.op()).await?;
        // op.commit().await?;
        Ok(())
    }
}
