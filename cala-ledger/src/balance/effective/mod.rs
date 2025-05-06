mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
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
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        _created_at: DateTime<Utc>,
        _account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
        balance_ids: (Vec<AccountId>, Vec<&str>),
    ) -> Result<(), BalanceError> {
        let mut all_datas = self
            .repo
            .find_for_update(db, journal_id, balance_ids, effective)
            .await?;
        for ((account_id, currency), data) in all_datas.iter_mut() {
            data.insert_entries(
                effective,
                entries
                    .iter()
                    .filter(|e| account_id == &e.account_id && currency == &e.currency),
            );
        }
        // let entries = self.entries.find_for_recalculating_effective().await?;
        //
        // all entries after effective <- sorted together with the new entries
        // -> derive snapshots from all of those entries
        // -> persist the snapshots

        Ok(())
    }
}
