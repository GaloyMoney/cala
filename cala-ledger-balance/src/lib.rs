#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

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
    balance::{BalanceAmount, BalanceSnapshot, JournalChecker, JournalInfo},
    entry::EntryValues,
    primitives::*,
};
use cala_ledger_outbox::*;

pub use account_balance::*;
use effective::*;
use error::BalanceError;
use repo::*;
pub use snapshot::*;

#[derive(Clone)]
pub struct Balances<J: JournalChecker> {
    repo: BalanceRepo,
    journals: J,
    effective: EffectiveBalances,
    _pool: PgPool,
}

impl<J: JournalChecker> Balances<J> {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher, journals: J) -> Self {
        Self {
            repo: BalanceRepo::new(pool, publisher),
            effective: EffectiveBalances::new(pool),
            journals,
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
    pub async fn update_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), BalanceError> {
        let journal_info = self
            .journals
            .check_journal(journal_id)
            .await
            .map_err(|e| BalanceError::JournalCheckError(Box::new(e)))?;
        if journal_info.is_locked {
            return Err(BalanceError::JournalLocked(journal_info.id));
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
            .find_for_update(op, journal_info.id, &all_involved_balances)
            .await?;
        let new_balances = Self::new_snapshots(
            created_at,
            current_balances,
            &entries,
            &account_set_mappings,
        );
        self.repo
            .insert_new_snapshots(op, journal_info.id, new_balances)
            .await?;

        if journal_info.enable_effective_balances {
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
    pub async fn find_balances_for_update(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, BalanceError> {
        self.repo
            .load_all_for_update(db, journal_id, account_id)
            .await
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

use cala_types::balance::BalanceProvider;

impl<J: JournalChecker> BalanceProvider for Balances<J> {
    type Error = BalanceError;

    async fn find_balances_for_update(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, Self::Error> {
        self.find_balances_for_update(db, journal_id, account_id)
            .await
    }

    async fn update_balances_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
        account_set_mappings: HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), Self::Error> {
        self.update_balances_in_op(db, journal_id, entries, effective, created_at, account_set_mappings)
            .await
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

        // Use a dummy JournalChecker for tests
        #[derive(Clone)]
        struct DummyJournalChecker;
        impl JournalChecker for DummyJournalChecker {
            type Error = std::io::Error;
            async fn check_journal(
                &self,
                _journal_id: JournalId,
            ) -> Result<JournalInfo, Self::Error> {
                unimplemented!()
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

            let result = Balances::<DummyJournalChecker>::new_snapshots(
                Utc::now(),
                current_balances,
                &entries,
                &HashMap::new(),
            );

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

            let result = Balances::<DummyJournalChecker>::new_snapshots(
                Utc::now(),
                current_balances,
                &entries,
                &HashMap::new(),
            );

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

            let result = Balances::<DummyJournalChecker>::new_snapshots(
                Utc::now(),
                current_balances,
                &entries,
                &HashMap::new(),
            );

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

            let result = Balances::<DummyJournalChecker>::new_snapshots(
                Utc::now(),
                current_balances,
                &entries,
                &HashMap::new(),
            );

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

            let result = Balances::<DummyJournalChecker>::new_snapshots(
                Utc::now(),
                current_balances,
                &entries,
                &mappings,
            );

            assert_eq!(result.len(), 2);
        }
    }
}
