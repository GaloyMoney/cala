mod account_balance;
mod effective;
pub mod error;
mod repo;
mod snapshot;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{BTreeSet, HashMap};
use tracing::instrument;

pub use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    journal::JournalValues,
};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{journal::Journals, outbox::*, primitives::JournalId};

pub use account_balance::*;
use effective::*;
use error::BalanceError;
use repo::*;
pub(crate) use snapshot::*;

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    journals: Journals,
    effective: EffectiveBalances,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher, journals: &Journals) -> Self {
        Self {
            repo: BalanceRepo::new(pool, publisher),
            effective: EffectiveBalances::new(pool),
            journals: journals.clone(),
            _pool: pool.clone(),
        }
    }

    pub fn effective(&self) -> &EffectiveBalances {
        &self.effective
    }

    #[instrument(name = "cala_ledger.balance.find", skip(self))]
    pub async fn find(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find(journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_in_op", skip(self, op))]
    pub async fn find_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find_in_op(op, journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_all", skip(self))]
    pub async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids).await
    }

    #[instrument(name = "cala_ledger.balance.find_all_in_op", skip(self, op))]
    pub async fn find_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all_in_op(op, ids).await
    }

    #[instrument(name = "cala_ledger.balance.update_balances_in_op", skip(self, op, entries, account_set_mappings), fields(journal_id = %journal_id, entries_count = entries.len()))]
    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), BalanceError> {
        let journal = self.journals.find(journal_id).await?;
        if journal.is_locked() {
            return Err(BalanceError::JournalLocked(journal.id));
        }

        // Using BTreeSet ensures consistent ordering of account/currency pairs
        // across all transactions. This prevents deadlocks when acquiring
        // advisory locks in find_for_update, as all transactions will attempt
        // to lock the same resources in the same order. Without this ordering,
        // concurrent transactions could acquire locks in different orders and
        // deadlock waiting for each other.
        let mut all_involved_balances: BTreeSet<_> = BTreeSet::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            all_involved_balances.extend(
                account_set_mappings
                    .get(&entry.account_id)
                    .unwrap_or(&empty)
                    .iter()
                    .map(AccountId::from)
                    .chain(std::iter::once(entry.account_id))
                    .map(|id| (id, entry.currency)),
            );
        }

        let all_involved_balances: (Vec<_>, Vec<_>) = all_involved_balances
            .into_iter()
            .map(|(a, c)| (a, c.code()))
            .unzip();

        let current_balances = self
            .repo
            .find_for_update(op, journal.id, &all_involved_balances)
            .await?;
        let new_balances = Self::new_snapshots(
            created_at,
            current_balances,
            &entries,
            &account_set_mappings,
        );
        self.repo
            .insert_new_snapshots(op, journal.id, new_balances)
            .await?;

        if journal.insert_effective_balances() {
            self.effective
                .update_cumulative_balances_in_op(
                    op,
                    journal_id,
                    entries,
                    effective,
                    created_at,
                    account_set_mappings,
                    all_involved_balances,
                )
                .await?;
        }

        Ok(())
    }

    #[instrument(name = "cala_ledger.balance.find_balances_for_update", skip(self, db), fields(journal_id = %journal_id, account_id = %account_id))]
    pub(crate) async fn find_balances_for_update(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, BalanceError> {
        self.repo
            .load_all_for_update(db, journal_id, account_id)
            .await
    }

    #[instrument(name = "cala_ledger.balance.update_balance_for_account_in_op", skip(self, op, entries), fields(journal_id = %journal_id, account_id = %account_id))]
    pub(crate) async fn update_balance_for_account_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
        entries: &[EntryValues],
        created_at: DateTime<Utc>,
    ) -> Result<(), BalanceError> {
        let current = self
            .repo
            .load_all_for_update(op, journal_id, account_id)
            .await?;
        let mut current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
            HashMap::new();
        for entry in entries {
            current_balances
                .entry((account_id, entry.currency))
                .or_insert_with(|| current.get(&entry.currency).cloned());
        }
        let new_balances =
            Self::new_snapshots(created_at, current_balances, entries, &HashMap::new());
        self.repo
            .insert_new_snapshots(op, journal_id, new_balances)
            .await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.balances.recalculate_account_set_balances_in_op",
        skip(self, op),
        fields(account_set_id = %account_set_id)
    )]
    pub(crate) async fn recalculate_account_set_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_set_id: AccountSetId,
    ) -> Result<(), BalanceError> {
        let account_id = AccountId::from(&account_set_id);

        let current_balances = self
            .repo
            .load_account_set_balances(op, journal_id, account_id)
            .await?;

        // Single watermark across all currencies is safe because EntryId is
        // UUID v7 (globally ordered by creation time). See doc comment on
        // fetch_incremental_member_history for the full invariant.
        let watermark = current_balances
            .values()
            .map(|snap| snap.entry_id)
            .max_by_key(|id| uuid::Uuid::from(*id))
            .filter(|id| uuid::Uuid::from(*id) != uuid::Uuid::nil());

        let new_history = self
            .repo
            .fetch_incremental_member_history(op, journal_id, account_set_id, watermark)
            .await?;

        if new_history.is_empty() {
            return Ok(());
        }

        let new_snapshots =
            Self::replay_member_deltas(journal_id, account_id, current_balances, new_history);

        if !new_snapshots.is_empty() {
            self.repo
                .insert_new_snapshots(op, journal_id, new_snapshots)
                .await?;
        }
        Ok(())
    }

    #[instrument(name = "cala_ledger.balances.replay_member_deltas", skip_all)]
    fn replay_member_deltas(
        journal_id: JournalId,
        account_id: AccountId,
        mut current_balances: HashMap<Currency, BalanceSnapshot>,
        history: Vec<MemberBalanceHistoryRow>,
    ) -> Vec<BalanceSnapshot> {
        use rust_decimal::Decimal;

        let mut new_snapshots = Vec::with_capacity(history.len());

        for MemberBalanceHistoryRow {
            snapshot,
            prev_snapshot,
        } in history
        {
            let (d_settled_dr, d_settled_cr, d_pending_dr, d_pending_cr, d_enc_dr, d_enc_cr) =
                match prev_snapshot {
                    Some(ref prev) => (
                        snapshot.settled.dr_balance - prev.settled.dr_balance,
                        snapshot.settled.cr_balance - prev.settled.cr_balance,
                        snapshot.pending.dr_balance - prev.pending.dr_balance,
                        snapshot.pending.cr_balance - prev.pending.cr_balance,
                        snapshot.encumbrance.dr_balance - prev.encumbrance.dr_balance,
                        snapshot.encumbrance.cr_balance - prev.encumbrance.cr_balance,
                    ),
                    None => (
                        snapshot.settled.dr_balance,
                        snapshot.settled.cr_balance,
                        snapshot.pending.dr_balance,
                        snapshot.pending.cr_balance,
                        snapshot.encumbrance.dr_balance,
                        snapshot.encumbrance.cr_balance,
                    ),
                };

            let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);
            let running = current_balances
                .entry(snapshot.currency)
                .or_insert_with(|| BalanceSnapshot {
                    journal_id,
                    account_id,
                    entry_id,
                    currency: snapshot.currency,
                    settled: BalanceAmount {
                        dr_balance: Decimal::ZERO,
                        cr_balance: Decimal::ZERO,
                        entry_id,
                        modified_at: snapshot.modified_at,
                    },
                    pending: BalanceAmount {
                        dr_balance: Decimal::ZERO,
                        cr_balance: Decimal::ZERO,
                        entry_id,
                        modified_at: snapshot.modified_at,
                    },
                    encumbrance: BalanceAmount {
                        dr_balance: Decimal::ZERO,
                        cr_balance: Decimal::ZERO,
                        entry_id,
                        modified_at: snapshot.modified_at,
                    },
                    version: 0,
                    modified_at: snapshot.modified_at,
                    created_at: snapshot.modified_at,
                });

            running.settled.dr_balance += d_settled_dr;
            running.settled.cr_balance += d_settled_cr;
            running.pending.dr_balance += d_pending_dr;
            running.pending.cr_balance += d_pending_cr;
            running.encumbrance.dr_balance += d_enc_dr;
            running.encumbrance.cr_balance += d_enc_cr;
            running.version += 1;
            running.entry_id = snapshot.entry_id;
            running.modified_at = snapshot.modified_at;

            if d_settled_dr != Decimal::ZERO || d_settled_cr != Decimal::ZERO {
                running.settled.entry_id = snapshot.settled.entry_id;
                running.settled.modified_at = snapshot.settled.modified_at;
            }
            if d_pending_dr != Decimal::ZERO || d_pending_cr != Decimal::ZERO {
                running.pending.entry_id = snapshot.pending.entry_id;
                running.pending.modified_at = snapshot.pending.modified_at;
            }
            if d_enc_dr != Decimal::ZERO || d_enc_cr != Decimal::ZERO {
                running.encumbrance.entry_id = snapshot.encumbrance.entry_id;
                running.encumbrance.modified_at = snapshot.encumbrance.modified_at;
            }

            new_snapshots.push(running.clone());
        }

        new_snapshots
    }

    #[instrument(name = "cala_ledger.balances.new_snapshots", skip_all)]
    fn new_snapshots(
        time: DateTime<Utc>,
        mut current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
        entries: &[EntryValues],
        mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Vec<BalanceSnapshot> {
        let mut latest_balances: HashMap<(AccountId, &Currency), BalanceSnapshot> = HashMap::new();
        let mut new_balances = Vec::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                let latest =
                    if let Some(latest) = latest_balances.remove(&(account_id, &entry.currency)) {
                        new_balances.push(latest.clone());
                        Some(latest)
                    } else {
                        None
                    };
                let current = current_balances.remove(&(account_id, entry.currency));
                let Some(balance) = latest.map(Some).or(current) else {
                    continue;
                };

                let new_snapshot = match balance {
                    Some(balance) => Snapshots::update_snapshot(time, balance, entry),
                    None => Snapshots::new_snapshot(time, account_id, entry),
                };

                latest_balances.insert((account_id, &entry.currency), new_snapshot);
            }
        }
        new_balances.extend(latest_balances.into_values());
        new_balances
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod new_snapshots {
        use super::*;

        use chrono::Utc;
        use rust_decimal::Decimal;
        use std::collections::HashMap;

        use cala_types::{
            balance::BalanceAmount,
            entry::EntryValues,
            primitives::{DebitOrCredit, Layer},
        };

        use crate::primitives::{Currency, EntryId, JournalId, TransactionId};

        fn create_test_entry(
            units: Decimal,
            direction: DebitOrCredit,
            layer: Layer,
            currency: &str,
            account_id: AccountId,
        ) -> EntryValues {
            EntryValues {
                id: EntryId::new(),
                version: 1,
                transaction_id: TransactionId::new(),
                journal_id: JournalId::new(),
                account_id,
                entry_type: "TEST_ENTRY".to_string(),
                sequence: 1,
                layer,
                currency: currency.parse().unwrap(),
                direction,
                units,
                description: None,
                metadata: None,
            }
        }

        fn create_test_balance_snapshot(
            account_id: AccountId,
            journal_id: JournalId,
            currency: Currency,
            version: u32,
        ) -> BalanceSnapshot {
            let time = Utc::now();
            let entry_id = EntryId::new();
            BalanceSnapshot {
                journal_id,
                account_id,
                entry_id,
                currency,
                settled: BalanceAmount {
                    dr_balance: Decimal::ZERO,
                    cr_balance: Decimal::ZERO,
                    entry_id,
                    modified_at: time,
                },
                pending: BalanceAmount {
                    dr_balance: Decimal::ZERO,
                    cr_balance: Decimal::ZERO,
                    entry_id,
                    modified_at: time,
                },
                encumbrance: BalanceAmount {
                    dr_balance: Decimal::ZERO,
                    cr_balance: Decimal::ZERO,
                    entry_id,
                    modified_at: time,
                },
                version,
                modified_at: time,
                created_at: time,
            }
        }

        #[test]
        fn new_snapshots_creates_new_snapshot_when_no_current_balance() {
            let account_id = AccountId::new();
            let currency: Currency = "USD".parse().unwrap();

            let entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), None);

            let entries = vec![entry];

            let result =
                Balances::new_snapshots(Utc::now(), current_balances, &entries, &HashMap::new());

            assert_eq!(result.len(), 1);
            let snapshot = &result[0];
            assert_eq!(snapshot.version, 1); // New snapshot starts at version 1
        }

        #[test]
        fn new_snapshots_updates_current_balance_with_entry() {
            let account_id = AccountId::new();
            let currency: Currency = "USD".parse().unwrap();

            let mut current_balances = HashMap::new();
            let version = 5;
            let mut current_balance =
                create_test_balance_snapshot(account_id, JournalId::new(), currency, version);
            current_balance.settled.dr_balance = Decimal::from(200);
            current_balance.settled.cr_balance = Decimal::from(50);
            current_balances.insert((account_id, currency), Some(current_balance));

            let entry = create_test_entry(
                Decimal::from(75),
                DebitOrCredit::Credit,
                Layer::Settled,
                "USD",
                account_id,
            );
            let entries = vec![entry];

            let result =
                Balances::new_snapshots(Utc::now(), current_balances, &entries, &HashMap::new());

            assert_eq!(result.len(), 1);
            let snapshot = &result[0];
            assert_eq!(snapshot.version, version + 1);
        }

        #[test]
        fn new_snapshots_can_update_from_multiple_entries() {
            let account_id = AccountId::new();
            let journal_id = JournalId::new();
            let currency: Currency = "USD".parse().unwrap();

            let initial_debit = Decimal::from(100);
            let initial_credit = Decimal::from(25);
            let mut current_balances = HashMap::new();
            let version = 3;
            let mut current_balance =
                create_test_balance_snapshot(account_id, journal_id, currency, version);
            current_balance.settled.dr_balance = initial_debit;
            current_balance.settled.cr_balance = initial_credit;
            current_balances.insert((account_id, currency), Some(current_balance));

            // First entry will create a latest balance
            let entry1_debit = Decimal::from(50);
            let entry1 = create_test_entry(
                entry1_debit,
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );
            // Second entry should use the latest balance, not the current
            let entry2_credit = Decimal::from(30);
            let entry2 = create_test_entry(
                entry2_credit,
                DebitOrCredit::Credit,
                Layer::Settled,
                "USD",
                account_id,
            );
            let entries = vec![entry1, entry2];

            let result =
                Balances::new_snapshots(Utc::now(), current_balances, &entries, &HashMap::new());

            assert_eq!(result.len(), 2);

            assert_eq!(result[0].version, version + 1);
            assert_eq!(result[0].settled.dr_balance, initial_debit + entry1_debit);

            assert_eq!(result[1].version, version + 2);
            assert_eq!(result[1].settled.cr_balance, initial_credit + entry2_credit);
        }

        #[test]
        fn new_snapshots_skips_update_when_no_balance_value_exists() {
            let current_balances = HashMap::new();

            let entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                AccountId::new(),
            );
            let entries = vec![entry];

            let result =
                Balances::new_snapshots(Utc::now(), current_balances, &entries, &HashMap::new());

            assert!(result.is_empty());
        }

        #[test]
        fn new_snapshots_creates_snapshots_for_mapped_account_sets() {
            let account_id = AccountId::new();
            let account_set_id = AccountSetId::new();
            let currency: Currency = "USD".parse().unwrap();

            let entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), None);
            current_balances.insert((AccountId::from(&account_set_id), currency), None);

            let mut mappings = HashMap::new();
            mappings.insert(account_id, vec![account_set_id]);

            let entries = vec![entry];

            let result = Balances::new_snapshots(Utc::now(), current_balances, &entries, &mappings);

            assert_eq!(result.len(), 2);
        }
    }
}
