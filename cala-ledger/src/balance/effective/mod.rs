mod data;
mod repo;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    entry::EntryValues,
    primitives::*,
};

use crate::primitives::JournalId;

use super::{account_balance::*, error::BalanceError, snapshot::UNASSIGNED_ENTRY_ID};

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

    #[instrument(
        name = "cala_ledger.balance.effective.recalculate_for_account_sets_in_op",
        skip(self, op)
    )]
    pub(crate) async fn recalculate_for_account_sets_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_set_ids: &[AccountSetId],
        memberships: &HashMap<AccountId, Vec<AccountSetId>>,
        min_watermark: Option<i64>,
    ) -> Result<(), BalanceError> {
        let history = self
            .repo
            .fetch_member_effective_history(&mut *op, journal_id, account_set_ids, min_watermark)
            .await?;

        if history.is_empty() {
            return Ok(());
        }

        let min_effective_date = history
            .iter()
            .map(|r| r.effective_date)
            .min()
            .expect("history is non-empty");

        let set_account_ids: Vec<AccountId> = account_set_ids.iter().map(AccountId::from).collect();

        self.repo
            .delete_at_or_after(&mut *op, journal_id, &set_account_ids, min_effective_date)
            .await?;

        let base_snapshots = self
            .repo
            .load_latest_before(&mut *op, journal_id, &set_account_ids, min_effective_date)
            .await?;

        let snapshots = Self::replay_effective_deltas(
            journal_id,
            account_set_ids,
            memberships,
            history,
            base_snapshots,
        );

        if !snapshots.is_empty() {
            self.repo
                .insert_recalc_snapshots(op, journal_id, snapshots)
                .await?;
        }

        Ok(())
    }

    #[instrument(
        name = "cala_ledger.balance.effective.replay_effective_deltas",
        skip_all
    )]
    fn replay_effective_deltas(
        journal_id: JournalId,
        account_set_ids: &[AccountSetId],
        memberships: &HashMap<AccountId, Vec<AccountSetId>>,
        history: Vec<EffectiveMemberHistoryRow>,
        base_snapshots: HashMap<(AccountId, Currency), (BalanceSnapshot, i32)>,
    ) -> Vec<RecalcEffectiveSnapshot> {
        use rust_decimal::Decimal;

        let set_ids: HashSet<&AccountSetId> = account_set_ids.iter().collect();

        struct RunningState {
            snapshot: BalanceSnapshot,
            last_date: Option<NaiveDate>,
            all_time_version: i32,
        }

        let mut states: HashMap<(AccountId, Currency), RunningState> = base_snapshots
            .into_iter()
            .map(|((account_id, currency), (snapshot, atv))| {
                (
                    (account_id, currency),
                    RunningState {
                        snapshot,
                        last_date: None,
                        all_time_version: atv,
                    },
                )
            })
            .collect();

        let mut result = Vec::new();

        for row in history {
            let (d_settled_dr, d_settled_cr, d_pending_dr, d_pending_cr, d_enc_dr, d_enc_cr) =
                match row.prev_snapshot {
                    Some(ref prev) => (
                        row.snapshot.settled.dr_balance - prev.settled.dr_balance,
                        row.snapshot.settled.cr_balance - prev.settled.cr_balance,
                        row.snapshot.pending.dr_balance - prev.pending.dr_balance,
                        row.snapshot.pending.cr_balance - prev.pending.cr_balance,
                        row.snapshot.encumbrance.dr_balance - prev.encumbrance.dr_balance,
                        row.snapshot.encumbrance.cr_balance - prev.encumbrance.cr_balance,
                    ),
                    None => (
                        row.snapshot.settled.dr_balance,
                        row.snapshot.settled.cr_balance,
                        row.snapshot.pending.dr_balance,
                        row.snapshot.pending.cr_balance,
                        row.snapshot.encumbrance.dr_balance,
                        row.snapshot.encumbrance.cr_balance,
                    ),
                };

            let empty = Vec::new();
            let owning_sets = memberships.get(&row.snapshot.account_id).unwrap_or(&empty);

            for set_id in owning_sets {
                if !set_ids.contains(set_id) {
                    continue;
                }

                let account_id = AccountId::from(set_id);
                let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);

                let state = states
                    .entry((account_id, row.snapshot.currency))
                    .or_insert_with(|| RunningState {
                        snapshot: BalanceSnapshot {
                            journal_id,
                            account_id,
                            entry_id,
                            currency: row.snapshot.currency,
                            settled: BalanceAmount {
                                dr_balance: Decimal::ZERO,
                                cr_balance: Decimal::ZERO,
                                entry_id,
                                modified_at: row.snapshot.modified_at,
                            },
                            pending: BalanceAmount {
                                dr_balance: Decimal::ZERO,
                                cr_balance: Decimal::ZERO,
                                entry_id,
                                modified_at: row.snapshot.modified_at,
                            },
                            encumbrance: BalanceAmount {
                                dr_balance: Decimal::ZERO,
                                cr_balance: Decimal::ZERO,
                                entry_id,
                                modified_at: row.snapshot.modified_at,
                            },
                            version: 0,
                            modified_at: row.snapshot.modified_at,
                            created_at: row.snapshot.modified_at,
                        },
                        last_date: None,
                        all_time_version: 0,
                    });

                if state.last_date != Some(row.effective_date) {
                    state.snapshot.version = 0;
                    state.last_date = Some(row.effective_date);
                }

                let running = &mut state.snapshot;
                running.settled.dr_balance += d_settled_dr;
                running.settled.cr_balance += d_settled_cr;
                running.pending.dr_balance += d_pending_dr;
                running.pending.cr_balance += d_pending_cr;
                running.encumbrance.dr_balance += d_enc_dr;
                running.encumbrance.cr_balance += d_enc_cr;
                running.version += 1;
                running.entry_id = row.snapshot.entry_id;
                running.modified_at = row.snapshot.modified_at;

                if d_settled_dr != Decimal::ZERO || d_settled_cr != Decimal::ZERO {
                    running.settled.entry_id = row.snapshot.settled.entry_id;
                    running.settled.modified_at = row.snapshot.settled.modified_at;
                }
                if d_pending_dr != Decimal::ZERO || d_pending_cr != Decimal::ZERO {
                    running.pending.entry_id = row.snapshot.pending.entry_id;
                    running.pending.modified_at = row.snapshot.pending.modified_at;
                }
                if d_enc_dr != Decimal::ZERO || d_enc_cr != Decimal::ZERO {
                    running.encumbrance.entry_id = row.snapshot.encumbrance.entry_id;
                    running.encumbrance.modified_at = row.snapshot.encumbrance.modified_at;
                }

                state.all_time_version += 1;

                result.push(RecalcEffectiveSnapshot {
                    account_id,
                    currency: row.snapshot.currency,
                    effective_date: row.effective_date,
                    snapshot: running.clone(),
                    all_time_version: state.all_time_version,
                });
            }
        }

        result
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
}
