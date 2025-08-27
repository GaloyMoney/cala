mod account_balance;
mod effective;
pub mod error;
mod repo;
mod snapshot;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::instrument;

pub use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    journal::JournalValues,
};
use cala_types::{entry::EntryValues, primitives::*};

use crate::{
    journal::Journals,
    ledger_operation::*,
    outbox::*,
    primitives::{DataSource, JournalId},
};

pub use account_balance::*;
use effective::*;
use error::BalanceError;
use repo::*;
pub(crate) use snapshot::*;

#[derive(Clone)]
pub struct Balances {
    repo: BalanceRepo,
    // Used only for "import" feature
    #[allow(dead_code)]
    outbox: Outbox,
    journals: Journals,
    effective: EffectiveBalances,
    _pool: PgPool,
}

impl Balances {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox, journals: &Journals) -> Self {
        Self {
            repo: BalanceRepo::new(pool),
            effective: EffectiveBalances::new(pool),
            outbox,
            journals: journals.clone(),
            _pool: pool.clone(),
        }
    }

    pub fn effective(&self) -> &EffectiveBalances {
        &self.effective
    }

    #[instrument(name = "cala_ledger.balance.find", skip(self), err)]
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

    #[instrument(name = "cala_ledger.balance.find_in_op", skip(self, op), err)]
    pub async fn find_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        currency: Currency,
    ) -> Result<AccountBalance, BalanceError> {
        self.repo
            .find_in_op(op, journal_id, account_id.into(), currency)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_all", skip(self), err)]
    pub async fn find_all(
        &self,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all(ids).await
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
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

        let mut all_involved_balances: HashSet<_> = entries
            .iter()
            .map(|entry| (entry.account_id, entry.currency))
            .collect();
        for entry in entries.iter() {
            if let Some(account_set_ids) = account_set_mappings.get(&entry.account_id) {
                all_involved_balances.extend(
                    account_set_ids
                        .iter()
                        .map(|account_set_id| (AccountId::from(account_set_id), entry.currency)),
                );
            }
        }

        let all_involved_balances: (Vec<_>, Vec<_>) = all_involved_balances
            .into_iter()
            .map(|(a, c)| (a, c.code()))
            .unzip();

        let new_balances = {
            let mut db = op.begin().await?;

            let current_balances = self
                .repo
                .find_for_update(&mut db, journal.id, &all_involved_balances)
                .await?;
            let new_balances = Self::new_snapshots(
                created_at,
                current_balances,
                &entries,
                &account_set_mappings,
            );
            self.repo
                .insert_new_snapshots(&mut db, journal.id, &new_balances)
                .await?;

            if journal.insert_effective_balances() {
                self.effective
                    .update_cumulative_balances_in_op(
                        &mut db,
                        journal_id,
                        entries,
                        effective,
                        created_at,
                        account_set_mappings,
                        all_involved_balances,
                    )
                    .await?;
            }

            db.commit().await?;

            new_balances
        };

        op.accumulate(new_balances.into_iter().map(|balance| {
            if balance.version == 1 {
                OutboxEventPayload::BalanceCreated {
                    source: DataSource::Local,
                    balance,
                }
            } else {
                OutboxEventPayload::BalanceUpdated {
                    source: DataSource::Local,
                    balance,
                }
            }
        }));
        Ok(())
    }

    pub(crate) async fn find_balances_for_update(
        &self,
        db: &mut LedgerOperation<'_>,
        journal_id: JournalId,
        account_id: AccountId,
    ) -> Result<HashMap<Currency, BalanceSnapshot>, BalanceError> {
        self.repo
            .load_all_for_update(db, journal_id, account_id)
            .await
    }

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
                let balance = if let Some(latest) =
                    latest_balances.remove(&(account_id, &entry.currency))
                {
                    new_balances.push(latest.clone());
                    Some(latest)
                } else if let Some(current) = current_balances.remove(&(account_id, entry.currency))
                {
                    current
                } else {
                    continue;
                };

                match balance {
                    Some(balance) => {
                        latest_balances.insert(
                            (account_id, &entry.currency),
                            Snapshots::update_snapshot(time, balance, entry),
                        );
                    }
                    None => {
                        latest_balances.insert(
                            (account_id, &entry.currency),
                            Snapshots::new_snapshot(time, account_id, entry),
                        );
                    }
                }
            }
        }
        new_balances.extend(latest_balances.into_values());
        new_balances
    }

    #[cfg(feature = "import")]
    pub async fn sync_balance_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo.import_balance(&mut db, &balance).await?;
        let recorded_at = balance.created_at;
        self.outbox
            .persist_events_at(
                db,
                std::iter::once(OutboxEventPayload::BalanceCreated {
                    source: DataSource::Remote { id: origin },
                    balance,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_balance_update(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        balance: BalanceSnapshot,
    ) -> Result<(), BalanceError> {
        self.repo.import_balance_update(&mut db, &balance).await?;
        let recorded_at = balance.modified_at;
        self.outbox
            .persist_events_at(
                db,
                std::iter::once(OutboxEventPayload::BalanceUpdated {
                    source: DataSource::Remote { id: origin },
                    balance,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
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
        fn new_snapshots_empty_entries_returns_empty() {
            let time = Utc::now();
            let current_balances = HashMap::new();
            let entries = vec![];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert!(result.is_empty());
        }

        #[test]
        fn new_snapshots_single_entry_no_existing_balance() {
            let time = Utc::now();
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
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 1);
            let snapshot = &result[0];
            assert_eq!(snapshot.account_id, account_id);
            assert_eq!(snapshot.currency, currency);
            assert_eq!(snapshot.version, 1);
            assert_eq!(snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshot.settled.cr_balance, Decimal::ZERO);
        }

        #[test]
        fn new_snapshots_single_entry_with_existing_balance() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let journal_id = JournalId::new();
            let currency: Currency = "USD".parse().unwrap();

            let mut existing_balance =
                create_test_balance_snapshot(account_id, journal_id, currency, 5);
            existing_balance.settled.dr_balance = Decimal::from(200);
            existing_balance.settled.cr_balance = Decimal::from(50);

            let entry = create_test_entry(
                Decimal::from(75),
                DebitOrCredit::Credit,
                Layer::Settled,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), Some(existing_balance));

            let entries = vec![entry];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 1);
            let snapshot = &result[0];
            assert_eq!(snapshot.version, 6); // Previous was 5
            assert_eq!(snapshot.settled.dr_balance, Decimal::from(200)); // Unchanged
            assert_eq!(snapshot.settled.cr_balance, Decimal::from(125)); // 50 + 75
        }

        #[test]
        fn new_snapshots_multiple_entries_same_account() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let currency: Currency = "USD".parse().unwrap();

            let entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );
            let entry2 = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Credit,
                Layer::Settled,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), None);

            let entries = vec![entry1, entry2];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 2);

            // First balance after first entry
            assert_eq!(result[0].version, 1);
            assert_eq!(result[0].settled.dr_balance, Decimal::from(100));
            assert_eq!(result[0].settled.cr_balance, Decimal::ZERO);

            // Second balance after second entry
            assert_eq!(result[1].version, 2);
            assert_eq!(result[1].settled.dr_balance, Decimal::from(100));
            assert_eq!(result[1].settled.cr_balance, Decimal::from(50));
        }

        #[test]
        fn new_snapshots_multiple_accounts() {
            let time = Utc::now();
            let account1 = AccountId::new();
            let account2 = AccountId::new();
            let currency: Currency = "USD".parse().unwrap();

            let entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account1,
            );
            let entry2 = create_test_entry(
                Decimal::from(200),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
                account2,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account1, currency), None);
            current_balances.insert((account2, currency), None);

            let entries = vec![entry1, entry2];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 2);

            // Find snapshots for each account
            let account1_snapshot = result.iter().find(|s| s.account_id == account1).unwrap();
            assert_eq!(account1_snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(account1_snapshot.settled.cr_balance, Decimal::ZERO);
            assert_eq!(account1_snapshot.pending.dr_balance, Decimal::ZERO);
            assert_eq!(account1_snapshot.pending.cr_balance, Decimal::ZERO);

            let account2_snapshot = result.iter().find(|s| s.account_id == account2).unwrap();
            assert_eq!(account2_snapshot.settled.dr_balance, Decimal::ZERO);
            assert_eq!(account2_snapshot.settled.cr_balance, Decimal::ZERO);
            assert_eq!(account2_snapshot.pending.dr_balance, Decimal::ZERO);
            assert_eq!(account2_snapshot.pending.cr_balance, Decimal::from(200));
        }

        #[test]
        fn new_snapshots_different_currencies() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let usd: Currency = "USD".parse().unwrap();
            let eur: Currency = "EUR".parse().unwrap();

            let entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );
            let entry2 = create_test_entry(
                Decimal::from(200),
                DebitOrCredit::Credit,
                Layer::Settled,
                "EUR",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, usd), None);
            current_balances.insert((account_id, eur), None);

            let entries = vec![entry1, entry2];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 2);

            let usd_snapshot = result.iter().find(|s| s.currency == usd).unwrap();
            assert_eq!(usd_snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(usd_snapshot.settled.cr_balance, Decimal::ZERO);

            let eur_snapshot = result.iter().find(|s| s.currency == eur).unwrap();
            assert_eq!(eur_snapshot.settled.dr_balance, Decimal::ZERO);
            assert_eq!(eur_snapshot.settled.cr_balance, Decimal::from(200));
        }

        #[test]
        fn new_snapshots_with_account_set_mappings() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let account_set_id1 = AccountSetId::new();
            let account_set_id2 = AccountSetId::new();
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
            current_balances.insert((AccountId::from(&account_set_id1), currency), None);
            current_balances.insert((AccountId::from(&account_set_id2), currency), None);

            let entries = vec![entry];
            let mut mappings = HashMap::new();
            mappings.insert(account_id, vec![account_set_id1, account_set_id2]);

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 3);

            // All balances should have the same debit amount
            for snapshot in &result {
                assert_eq!(snapshot.settled.dr_balance, Decimal::from(100));
                assert_eq!(snapshot.settled.cr_balance, Decimal::ZERO);
                assert_eq!(snapshot.version, 1);
            }
        }

        #[test]
        fn new_snapshots_different_layers() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let currency: Currency = "USD".parse().unwrap();

            let entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );
            let entry2 = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
                account_id,
            );
            let entry3 = create_test_entry(
                Decimal::from(25),
                DebitOrCredit::Debit,
                Layer::Encumbrance,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), None);

            let entries = vec![entry1, entry2, entry3];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 3);

            // Final snapshot should have all layers updated
            let final_snapshot = &result[2];
            assert_eq!(final_snapshot.version, 3);
            assert_eq!(final_snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(final_snapshot.settled.cr_balance, Decimal::ZERO);
            assert_eq!(final_snapshot.pending.dr_balance, Decimal::ZERO);
            assert_eq!(final_snapshot.pending.cr_balance, Decimal::from(50));
            assert_eq!(final_snapshot.encumbrance.dr_balance, Decimal::from(25));
            assert_eq!(final_snapshot.encumbrance.cr_balance, Decimal::ZERO);
        }

        #[test]
        fn new_snapshots_preserves_other_layer_balances() {
            let time = Utc::now();
            let account_id = AccountId::new();
            let journal_id = JournalId::new();
            let currency: Currency = "USD".parse().unwrap();

            let mut existing_balance =
                create_test_balance_snapshot(account_id, journal_id, currency, 2);
            existing_balance.settled.dr_balance = Decimal::from(100);
            existing_balance.pending.cr_balance = Decimal::from(50);
            existing_balance.encumbrance.dr_balance = Decimal::from(25);

            let entry = create_test_entry(
                Decimal::from(30),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
                account_id,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account_id, currency), Some(existing_balance));

            let entries = vec![entry];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert_eq!(result.len(), 1);
            let snapshot = &result[0];
            assert_eq!(snapshot.version, 3);
            // Settled layer unchanged
            assert_eq!(snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshot.settled.cr_balance, Decimal::ZERO);
            // Pending layer updated
            assert_eq!(snapshot.pending.dr_balance, Decimal::ZERO);
            assert_eq!(snapshot.pending.cr_balance, Decimal::from(80)); // 50 + 30
                                                                        // Encumbrance layer unchanged
            assert_eq!(snapshot.encumbrance.dr_balance, Decimal::from(25));
            assert_eq!(snapshot.encumbrance.cr_balance, Decimal::ZERO);
        }

        #[test]
        fn new_snapshots_missing_balance_in_current_balances_skipped() {
            let time = Utc::now();
            let account_id = AccountId::new();

            let entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account_id,
            );

            let current_balances = HashMap::new();
            // Note: NOT adding the balance to current_balances

            let entries = vec![entry];
            let mappings = HashMap::new();

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            assert!(result.is_empty());
        }

        #[test]
        fn new_snapshots_complex_scenario_multiple_accounts_and_mappings() {
            let time = Utc::now();
            let account1 = AccountId::new();
            let account2 = AccountId::new();
            let account_set1 = AccountSetId::new();
            let account_set2 = AccountSetId::new();
            let usd: Currency = "USD".parse().unwrap();
            let eur: Currency = "EUR".parse().unwrap();

            let entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
                account1,
            );
            let entry2 = create_test_entry(
                Decimal::from(200),
                DebitOrCredit::Credit,
                Layer::Pending,
                "EUR",
                account2,
            );
            let entry3 = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Debit,
                Layer::Encumbrance,
                "USD",
                account1,
            );

            let mut current_balances = HashMap::new();
            current_balances.insert((account1, usd), None);
            current_balances.insert((account2, eur), None);
            current_balances.insert((AccountId::from(&account_set1), usd), None);
            current_balances.insert((AccountId::from(&account_set2), usd), None);

            let entries = vec![entry1, entry2, entry3];
            let mut mappings = HashMap::new();
            mappings.insert(account1, vec![account_set1, account_set2]);

            let result = Balances::new_snapshots(time, current_balances, &entries, &mappings);

            // Should have:
            // - 2 snapshots for account1 (settled + encumbrance entries)
            // - 1 snapshot for account2 (pending entry)
            // - 2 snapshots for account_set1 (mapped from account1, both entries)
            // - 2 snapshots for account_set2 (mapped from account1, both entries)
            assert_eq!(result.len(), 7);

            // Check account1 final state
            let account1_snapshots: Vec<_> = result
                .iter()
                .filter(|s| s.account_id == account1 && s.currency == usd)
                .collect();
            assert_eq!(account1_snapshots.len(), 2);
            let final_account1 = account1_snapshots.last().unwrap();
            assert_eq!(final_account1.settled.dr_balance, Decimal::from(100));
            assert_eq!(final_account1.encumbrance.dr_balance, Decimal::from(50));

            // Check account2 state
            let account2_snapshot = result.iter().find(|s| s.account_id == account2).unwrap();
            assert_eq!(account2_snapshot.pending.cr_balance, Decimal::from(200));

            // Check account sets have same balances as account1
            for account_set_id in [account_set1, account_set2] {
                let set_snapshots: Vec<_> = result
                    .iter()
                    .filter(|s| s.account_id == AccountId::from(&account_set_id))
                    .collect();
                assert_eq!(set_snapshots.len(), 2);
                let final_set = set_snapshots.last().unwrap();
                assert_eq!(final_set.settled.dr_balance, Decimal::from(100));
                assert_eq!(final_set.encumbrance.dr_balance, Decimal::from(50));
            }
        }
    }
}
