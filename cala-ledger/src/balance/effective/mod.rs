mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use cala_types::{balance::EffectiveBalanceSnapshot, entry::EntryValues, primitives::*};

use crate::{outbox::OutboxPublisher, primitives::JournalId};

use data::EffectiveBalanceData;

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

    #[instrument(name = "cala_ledger.balance.effective.find_cumulative", skip(self))]
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

    #[instrument(name = "cala_ledger.balance.effective.find_in_range", skip(self))]
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

    #[instrument(name = "cala_ledger.balance.effective.find_all_cumulative", skip(self))]
    pub async fn find_all_cumulative(
        &self,
        ids: &[BalanceId],
        date: NaiveDate,
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids, date).await
    }

    #[instrument(
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
        name = "cala_ledger.balance.effective.list_cumulative_for_accounts",
        skip(self)
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

    #[instrument(name = "cala_ledger.balance.effective.find_all_in_range", skip(self))]
    pub async fn find_all_in_range(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<BalanceId, BalanceRange>, BalanceError> {
        let ranges = self.repo.find_range_all(ids, from, until).await?;
        Ok(Self::balance_ranges_from_snapshots(ranges))
    }

    #[instrument(
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
        name = "cala_ledger.balance.effective.list_in_range_for_accounts",
        skip(self)
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

    fn balance_ranges_from_snapshots(
        ranges: HashMap<BalanceId, (Option<AccountBalance>, u32, Option<AccountBalance>, u32)>,
    ) -> HashMap<BalanceId, BalanceRange> {
        let mut res = HashMap::new();
        for (id, (start, start_version, end, end_version)) in ranges {
            if let Some(end) = end {
                res.insert(
                    id,
                    BalanceRange::new(start, end, end_version - start_version),
                );
            }
        }
        res
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

        let new_balances = Self::new_effective_snapshots(journal_id, all_data);
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }

    /// EC-set counterpart of [`Self::update_cumulative_balances_in_op`]
    /// used by the streaming rollup: fans each entry into its EC ancestor
    /// sets only (never the leaf account), reading via `find_ec_for_update`
    /// which keeps the `eventually_consistent = TRUE` rows.
    #[allow(clippy::too_many_arguments)]
    #[instrument(
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
            {
                if let Some(data) = all_data.get_mut(&(account_id, entry.currency)) {
                    data.push(effective, entry);
                }
            }
        }
        for data in all_data.values_mut() {
            data.re_calculate_snapshots(created_at);
        }

        let new_balances = Self::new_effective_snapshots(journal_id, all_data);
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;

        Ok(())
    }

    fn new_effective_snapshots(
        journal_id: JournalId,
        data: HashMap<(AccountId, Currency), EffectiveBalanceData<'_>>,
    ) -> Vec<EffectiveBalanceSnapshot> {
        data.into_values()
            .flat_map(|d| d.into_updates())
            .map(
                |(account_id, currency, effective, snapshot, all_time_version)| {
                    EffectiveBalanceSnapshot {
                        journal_id,
                        account_id,
                        currency,
                        effective,
                        version: snapshot.version,
                        all_time_version,
                        created_at: snapshot.created_at,
                        modified_at: snapshot.modified_at,
                        entry_id: snapshot.entry_id,
                        settled: snapshot.settled,
                        pending: snapshot.pending,
                        encumbrance: snapshot.encumbrance,
                    }
                },
            )
            .collect()
    }
}
