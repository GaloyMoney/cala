mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

use cala_types::{balance::EffectiveBalanceSnapshot, entry::EntryValues, primitives::*};

use crate::{outbox::OutboxPublisher, primitives::JournalId};

use super::{
    account_balance::*,
    cursor::{AccountBalanceByCurrencyCursor, AccountBalanceCursor},
    error::BalanceError,
};

use repo::*;

#[derive(Clone)]
pub struct EffectiveBalances {
    repo: EffectiveBalanceRepo,
    _pool: PgPool,
}
impl EffectiveBalances {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            repo: EffectiveBalanceRepo::new(pool, publisher),
            _pool: pool.clone(),
        }
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_cumulative",
        skip(self)
    )]
    pub async fn find_cumulative(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
        date: NaiveDate,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find(journal_id, account_id.into(), currency, date)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.find_in_range",
        skip(self)
    )]
    pub async fn find_in_range(
        &self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<BalanceRange, BalanceError> {
        match self
            .repo
            .find_range(journal_id, account_id, currency, from, until)
            .await?
        {
            (start, Some(end), version_diff) => Ok(BalanceRange::new(start, end, version_diff)),
            _ => Err(BalanceError::NotFound(journal_id, account_id, currency)),
        }
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_cumulative", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_cumulative(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids, date).await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_cumulative_for_account",
        skip(self)
    )]
    pub async fn list_cumulative_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account(journal_id, account_id.into(), date, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_cumulative_for_accounts",
        skip(self, account_ids),
        fields(account_ids_count = account_ids.len())
    )]
    pub async fn list_cumulative_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        date: NaiveDate,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts(journal_id, account_ids, date, args)
            .await
    }

    #[instrument(level = "debug", name = "cala_ledger.balance.effective.find_all_in_range", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all_in_range(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<BalanceId, BalanceRange>, BalanceError> {
        let ranges = self.repo.find_range_all(ids, from, until).await?;
        Ok(ranges
            .into_iter()
            .filter_map(|(id, (start, start_version, end, end_version))| {
                BalanceRange::from_bounds(start, start_version, end, end_version)
                    .map(|range| (id, range))
            })
            .collect())
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_in_range_for_account",
        skip(self)
    )]
    pub async fn list_in_range_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_range_for_account(journal_id, account_id.into(), from, until, args)
            .await
    }

    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.list_in_range_for_accounts",
        skip(self, account_ids),
        fields(account_ids_count = account_ids.len())
    )]
    pub async fn list_in_range_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        from: NaiveDate,
        until: Option<NaiveDate>,
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<BalanceRange, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_range_for_accounts(journal_id, account_ids, from, until, args)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_cumulative_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        mappings: HashMap<AccountId, Vec<AccountSetId>>,
        balance_ids: (Vec<AccountId>, Vec<&str>),
    ) -> Result<(), BalanceError> {
        let mut all_data = self
            .repo
            .find_for_update(&mut *op, journal_id, balance_ids, effective)
            .await?;
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                if let Some(data) = all_data.get_mut(&(account_id, entry.currency)) {
                    data.push(effective, entry);
                }
            }
        }
        for data in all_data.values_mut() {
            data.re_calculate_snapshots(created_at);
        }

        let new_balances = all_data
            .into_values()
            .flat_map(|data| data.into_snapshots(journal_id))
            .collect();
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }

    /// EC counterpart of [`Self::update_cumulative_balances_in_op`] used by
    /// the streaming rollup: fans each entry into its EC ancestor sets and,
    /// for an entry whose leaf is an EC plain account (listed in
    /// `ec_leaves`), into that leaf's own cumulative-effective balance too.
    /// Reads via `find_ec_for_update`, which keeps the
    /// `eventually_consistent = TRUE` rows (both EC sets and EC leaves).
    #[allow(clippy::too_many_arguments)]
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.apply_ec_rollup_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(crate) async fn apply_ec_rollup_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        ec_mappings: HashMap<AccountId, Vec<AccountSetId>>,
        balance_ids: (Vec<AccountId>, Vec<&str>),
        ec_leaves: &HashSet<AccountId>,
    ) -> Result<(), BalanceError> {
        let mut all_data = self
            .repo
            .find_ec_for_update(&mut *op, journal_id, balance_ids, effective)
            .await?;
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in ec_mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(ec_leaves.get(&entry.account_id).copied())
            {
                if let Some(data) = all_data.get_mut(&(account_id, entry.currency)) {
                    data.push(effective, entry);
                }
            }
        }
        for data in all_data.values_mut() {
            data.re_calculate_snapshots(created_at);
        }

        let new_balances = all_data
            .into_values()
            .flat_map(|data| data.into_snapshots(journal_id))
            .collect();
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }

    /// Repair path used by the EC reconciler (`Balances::repair_ec`):
    /// drop every cumulative-effective row of the drifted
    /// `(account, currency)` pairs and reinsert the rebuilt series. The
    /// cumulative table is a projection (the inline back-dating path
    /// already deletes future-dated rows), so a wholesale rebuild is
    /// legitimate; reinserted rows publish through the normal
    /// `insert_new_snapshots` outbox path.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balance.effective.rebuild_ec_series_in_op",
        skip_all,
        fields(pairs = pairs.len(), snapshots = snapshots.len()),
        err(level = "warn")
    )]
    pub(crate) async fn rebuild_ec_series_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        pairs: &[(AccountId, Currency)],
        snapshots: Vec<EffectiveBalanceSnapshot>,
    ) -> Result<(), BalanceError> {
        self.repo.delete_for_pairs(op, journal_id, pairs).await?;
        if !snapshots.is_empty() {
            self.repo
                .insert_new_snapshots(op, journal_id, snapshots)
                .await?;
        }
        Ok(())
    }
}
