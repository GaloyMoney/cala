mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use cala_types::{entry::EntryValues, primitives::*};

use crate::primitives::JournalId;

use super::{account_balance::*, error::BalanceError};

use data::*;
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

    #[instrument(name = "cala_ledger.balance.effective.find_all_in_range", skip(self))]
    pub async fn find_all_in_range(
        &self,
        ids: &[BalanceId],
        from: NaiveDate,
        until: Option<NaiveDate>,
    ) -> Result<HashMap<BalanceId, BalanceRange>, BalanceError> {
        let ranges = self.repo.find_range_all(ids, from, until).await?;

        let mut res = HashMap::new();
        for (id, (start, start_version, end, end_version)) in ranges {
            if let Some(end) = end {
                res.insert(
                    id,
                    BalanceRange::new(start, end, end_version - start_version),
                );
            }
        }
        Ok(res)
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

        self.repo
            .insert_new_snapshots(op, journal_id, all_data)
            .await?;

        Ok(())
    }

    #[instrument(
        name = "cala_ledger.balance.effective.enqueue_recalculation",
        skip(self, op)
    )]
    pub(crate) async fn enqueue_recalculation_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        effective: NaiveDate,
        balance_ids: &(Vec<AccountId>, Vec<&str>),
    ) -> Result<(), BalanceError> {
        self.repo
            .enqueue_recalculation(op, journal_id, balance_ids, effective)
            .await
    }

    #[instrument(
        name = "cala_ledger.balance.effective.enqueue_ec_recalculation",
        skip(self, op)
    )]
    pub(crate) async fn enqueue_ec_recalculation_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        effective: NaiveDate,
        balance_ids: &(Vec<AccountId>, Vec<&str>),
    ) -> Result<(), BalanceError> {
        self.repo
            .enqueue_ec_recalculation(op, journal_id, balance_ids, effective)
            .await
    }

    #[instrument(
        name = "cala_ledger.balance.effective.process_recalc_queue",
        skip(self, op)
    )]
    pub(crate) async fn process_recalc_queue_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        limit: i32,
    ) -> Result<u32, BalanceError> {
        let queue_entries = self.repo.pop_recalc_queue(&mut *op, limit).await?;
        let count = queue_entries.len() as u32;

        for queue_entry in queue_entries {
            let (base_snapshot, base_version) = self
                .repo
                .delete_and_load_base(
                    &mut *op,
                    queue_entry.journal_id,
                    queue_entry.account_id,
                    queue_entry.currency.code(),
                    queue_entry.earliest_effective_date,
                )
                .await?;

            let loaded_entries = self
                .repo
                .load_entries_for_recalculation(
                    &mut *op,
                    queue_entry.journal_id,
                    queue_entry.account_id,
                    queue_entry.currency.code(),
                    queue_entry.earliest_effective_date,
                )
                .await?;

            if loaded_entries.is_empty() {
                continue;
            }

            let mut data = EffectiveBalanceData::new(
                queue_entry.account_id,
                queue_entry.currency,
                base_snapshot,
                base_version,
                Vec::new(),
            );

            for (effective_date, entry_values) in loaded_entries.iter() {
                data.push(*effective_date, entry_values);
            }

            data.re_calculate_snapshots(Utc::now());

            let mut all_data = HashMap::new();
            all_data.insert((queue_entry.account_id, queue_entry.currency), data);
            self.repo
                .insert_new_snapshots(&mut *op, queue_entry.journal_id, all_data)
                .await?;
        }

        Ok(count)
    }
}
