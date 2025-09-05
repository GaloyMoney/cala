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
