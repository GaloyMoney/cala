use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    entry::*,
    primitives::{DebitOrCredit, Layer},
};

use tracing::instrument;

use crate::primitives::{AccountId, AccountSetId, Currency, EntryId};
use std::collections::{HashMap, HashSet};

pub(super) const UNASSIGNED_ENTRY_ID: uuid::Uuid = uuid::Uuid::nil();

pub(crate) struct Snapshots;

impl Snapshots {
    pub(crate) fn new_snapshot(
        time: DateTime<Utc>,
        account_id: AccountId,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);
        Self::update_snapshot(
            time,
            BalanceSnapshot {
                journal_id: entry.journal_id,
                account_id,
                entry_id,
                currency: entry.currency,
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
                version: 0,
                modified_at: time,
                created_at: time,
            },
            entry,
        )
    }

    pub(crate) fn update_snapshot(
        time: DateTime<Utc>,
        mut snapshot: BalanceSnapshot,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        snapshot.version += 1;
        snapshot.modified_at = time;
        snapshot.entry_id = entry.id;
        match entry.layer {
            Layer::Settled => {
                snapshot.settled.entry_id = entry.id;
                snapshot.settled.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.settled.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.settled.cr_balance += entry.units;
                    }
                }
            }
            Layer::Pending => {
                snapshot.pending.entry_id = entry.id;
                snapshot.pending.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.pending.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.pending.cr_balance += entry.units;
                    }
                }
            }
            Layer::Encumbrance => {
                snapshot.encumbrance.entry_id = entry.id;
                snapshot.encumbrance.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.encumbrance.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.encumbrance.cr_balance += entry.units;
                    }
                }
            }
        }
        snapshot
    }

    /// Build a chain of balance snapshots from a batch of entries.
    ///
    /// For each entry the delta is folded into:
    /// - the leaf account itself, and
    /// - every ancestor account set in `account_set_mappings`.
    ///
    /// Repeated writes to the same `(account, currency)` within the batch
    /// are chained so each gets a new version and the final snapshot
    /// reflects the cumulative delta.
    #[instrument(level = "debug", name = "cala_ledger.balances.from_entries", skip_all)]
    pub(crate) fn from_entries(
        time: DateTime<Utc>,
        current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
        entries: &[EntryValues],
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Vec<BalanceSnapshot> {
        let mut fold = SnapshotFold::new(time, current_balances);
        for entry in entries.iter() {
            for set in account_set_mappings
                .get(&entry.account_id)
                .into_iter()
                .flatten()
                .map(AccountId::from)
            {
                fold.apply(set, entry);
            }
            // Leaf account balance is maintained on this inline path.
            fold.apply(entry.account_id, entry);
        }
        fold.into_snapshots()
    }

    /// Streaming-rollup counterpart of [`Self::from_entries`]: fans each entry
    /// into its eventually-consistent ancestor account sets, plus — when the
    /// leaf is an EC plain account (in `ec_leaves`) — the leaf's own balance.
    /// A synchronous leaf is left to the inline poster; an EC leaf is skipped
    /// by the poster, so the rollup is its sole writer.
    #[instrument(
        level = "debug",
        name = "cala_ledger.balances.from_ec_entries",
        skip_all
    )]
    pub(crate) fn from_ec_entries(
        time: DateTime<Utc>,
        current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
        entries: &[EntryValues],
        ec_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        ec_leaves: &HashSet<AccountId>,
    ) -> Vec<BalanceSnapshot> {
        let mut fold = SnapshotFold::new(time, current_balances);
        for entry in entries.iter() {
            for set in ec_mappings
                .get(&entry.account_id)
                .into_iter()
                .flatten()
                .map(AccountId::from)
            {
                fold.apply(set, entry);
            }
            if ec_leaves.contains(&entry.account_id) {
                fold.apply(entry.account_id, entry);
            }
        }
        fold.into_snapshots()
    }
}

/// Mutable accumulator that folds a batch of entry deltas into a chain of
/// balance snapshots, versioning repeated writes to the same
/// `(account, currency)` within the batch.
struct SnapshotFold<'a> {
    time: DateTime<Utc>,
    current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
    latest: HashMap<(AccountId, &'a Currency), BalanceSnapshot>,
    completed: Vec<BalanceSnapshot>,
}

impl<'a> SnapshotFold<'a> {
    fn new(
        time: DateTime<Utc>,
        current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
    ) -> Self {
        Self {
            time,
            current,
            latest: HashMap::new(),
            completed: Vec::new(),
        }
    }

    /// Fold one entry's delta into `account_id`, chaining onto any prior
    /// snapshot already written for that `(account, currency)` this batch.
    fn apply(&mut self, account_id: AccountId, entry: &'a EntryValues) {
        let base = if let Some(prev) = self.latest.remove(&(account_id, &entry.currency)) {
            // Already touched this batch: persist the intermediate, chain on.
            self.completed.push(prev.clone());
            Some(prev)
        } else {
            // First touch: fall back to the pre-loaded balance. A key that
            // was never loaded is not involved — skip it.
            match self.current.remove(&(account_id, entry.currency)) {
                Some(loaded) => loaded,
                None => return,
            }
        };

        let next = match base {
            Some(balance) => Snapshots::update_snapshot(self.time, balance, entry),
            None => Snapshots::new_snapshot(self.time, account_id, entry),
        };
        self.latest.insert((account_id, &entry.currency), next);
    }

    fn into_snapshots(mut self) -> Vec<BalanceSnapshot> {
        self.completed.extend(self.latest.into_values());
        self.completed
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
                Snapshots::from_entries(Utc::now(), current_balances, &entries, &HashMap::new());

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
                Snapshots::from_entries(Utc::now(), current_balances, &entries, &HashMap::new());

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
                Snapshots::from_entries(Utc::now(), current_balances, &entries, &HashMap::new());

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
                Snapshots::from_entries(Utc::now(), current_balances, &entries, &HashMap::new());

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

            let result = Snapshots::from_entries(Utc::now(), current_balances, &entries, &mappings);

            assert_eq!(result.len(), 2);
        }
    }

    mod ec_set_snapshots {
        use super::*;

        use chrono::Utc;
        use rust_decimal::Decimal;
        use std::collections::{HashMap, HashSet};

        use cala_types::{
            entry::EntryValues,
            primitives::{DebitOrCredit, Layer},
        };

        use crate::primitives::{AccountSetId, Currency, EntryId, JournalId, TransactionId};

        fn no_leaves() -> HashSet<AccountId> {
            HashSet::new()
        }

        fn credit_entry(units: Decimal, account_id: AccountId) -> EntryValues {
            EntryValues {
                id: EntryId::new(),
                version: 1,
                transaction_id: TransactionId::new(),
                journal_id: JournalId::new(),
                account_id,
                entry_type: "TEST_ENTRY".to_string(),
                sequence: 1,
                layer: Layer::Settled,
                currency: "USD".parse().unwrap(),
                direction: DebitOrCredit::Credit,
                units,
                description: None,
                metadata: None,
            }
        }

        fn balance_snapshot(
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
        fn fans_member_deltas_into_ec_ancestor_only() {
            let usd: Currency = "USD".parse().unwrap();
            let set_id = AccountSetId::new();
            let set_account = AccountId::from(&set_id);
            let m1 = AccountId::new();
            let m2 = AccountId::new();

            let entries = vec![
                credit_entry(Decimal::from(100), m1),
                credit_entry(Decimal::from(50), m2),
            ];

            let mut ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            ec_mappings.insert(m1, vec![set_id]);
            ec_mappings.insert(m2, vec![set_id]);

            // EC set has no prior balance.
            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            current.insert((set_account, usd), None);

            let snapshots = Snapshots::from_ec_entries(
                Utc::now(),
                current,
                &entries,
                &ec_mappings,
                &no_leaves(),
            );

            // Only the EC set is written — never the leaf accounts.
            assert!(snapshots.iter().all(|s| s.account_id == set_account));
            // The final (highest-version) snapshot reflects both credits.
            let final_snapshot = snapshots.iter().max_by_key(|s| s.version).unwrap();
            assert_eq!(final_snapshot.settled.cr_balance, Decimal::from(150));
            assert_eq!(final_snapshot.version, 2);
        }

        #[test]
        fn updates_existing_ec_set_balance() {
            let usd: Currency = "USD".parse().unwrap();
            let set_id = AccountSetId::new();
            let set_account = AccountId::from(&set_id);
            let member = AccountId::new();

            let entries = vec![credit_entry(Decimal::from(75), member)];

            let mut ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            ec_mappings.insert(member, vec![set_id]);

            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            let version = 3;
            let mut prior = balance_snapshot(set_account, JournalId::new(), usd, version);
            prior.settled.dr_balance = Decimal::from(200);
            prior.settled.cr_balance = Decimal::from(50);
            current.insert((set_account, usd), Some(prior));

            let snapshots = Snapshots::from_ec_entries(
                Utc::now(),
                current,
                &entries,
                &ec_mappings,
                &no_leaves(),
            );

            assert_eq!(snapshots.len(), 1);
            let snapshot = &snapshots[0];
            assert_eq!(snapshot.account_id, set_account);
            assert_eq!(snapshot.version, version + 1);
            assert_eq!(snapshot.settled.dr_balance, Decimal::from(200));
            assert_eq!(snapshot.settled.cr_balance, Decimal::from(125));
        }

        #[test]
        fn fans_single_member_into_multiple_ec_sets() {
            let usd: Currency = "USD".parse().unwrap();
            let set_a_id = AccountSetId::new();
            let set_a_account = AccountId::from(&set_a_id);
            let set_b_id = AccountSetId::new();
            let set_b_account = AccountId::from(&set_b_id);
            let member = AccountId::new();

            let entries = vec![credit_entry(Decimal::from(100), member)];

            let mut ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            ec_mappings.insert(member, vec![set_a_id, set_b_id]);

            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            current.insert((set_a_account, usd), None);
            current.insert((set_b_account, usd), None);

            let snapshots = Snapshots::from_ec_entries(
                Utc::now(),
                current,
                &entries,
                &ec_mappings,
                &no_leaves(),
            );

            assert_eq!(snapshots.len(), 2);
            assert!(snapshots.iter().all(|s| s.version == 1));
            assert!(snapshots
                .iter()
                .all(|s| s.settled.cr_balance == Decimal::from(100)));
            assert!(snapshots.iter().any(|s| s.account_id == set_a_account));
            assert!(snapshots.iter().any(|s| s.account_id == set_b_account));
            assert!(snapshots.iter().all(|s| s.account_id != member));
        }

        #[test]
        fn skips_members_without_ec_ancestors() {
            let entries = vec![credit_entry(Decimal::from(10), AccountId::new())];
            let ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            let current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> = HashMap::new();

            let snapshots = Snapshots::from_ec_entries(
                Utc::now(),
                current,
                &entries,
                &ec_mappings,
                &no_leaves(),
            );
            assert!(snapshots.is_empty());
        }

        #[test]
        fn folds_ec_leaf_and_its_ec_ancestor() {
            let usd: Currency = "USD".parse().unwrap();
            let set_id = AccountSetId::new();
            let set_account = AccountId::from(&set_id);
            // An EC plain-account leaf that is also a member of an EC set.
            let leaf = AccountId::new();

            let entries = vec![
                credit_entry(Decimal::from(40), leaf),
                credit_entry(Decimal::from(60), leaf),
            ];

            let mut ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            ec_mappings.insert(leaf, vec![set_id]);
            let ec_leaves: HashSet<AccountId> = [leaf].into_iter().collect();

            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            current.insert((set_account, usd), None);
            current.insert((leaf, usd), None);

            let snapshots =
                Snapshots::from_ec_entries(Utc::now(), current, &entries, &ec_mappings, &ec_leaves);

            // Both the leaf and its EC ancestor are written — and only those.
            let leaf_final = snapshots
                .iter()
                .filter(|s| s.account_id == leaf)
                .max_by_key(|s| s.version)
                .expect("leaf balance must be folded");
            let set_final = snapshots
                .iter()
                .filter(|s| s.account_id == set_account)
                .max_by_key(|s| s.version)
                .expect("EC ancestor balance must be folded");
            assert_eq!(leaf_final.settled.cr_balance, Decimal::from(100));
            assert_eq!(leaf_final.version, 2);
            assert_eq!(set_final.settled.cr_balance, Decimal::from(100));
            assert_eq!(set_final.version, 2);
            assert!(snapshots
                .iter()
                .all(|s| s.account_id == leaf || s.account_id == set_account));
        }

        #[test]
        fn folds_standalone_ec_leaf_without_ancestors() {
            let usd: Currency = "USD".parse().unwrap();
            let leaf = AccountId::new();
            let entries = vec![credit_entry(Decimal::from(25), leaf)];

            // No EC ancestor sets at all — only the leaf itself is EC.
            let ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            let ec_leaves: HashSet<AccountId> = [leaf].into_iter().collect();

            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            current.insert((leaf, usd), None);

            let snapshots =
                Snapshots::from_ec_entries(Utc::now(), current, &entries, &ec_mappings, &ec_leaves);

            assert_eq!(snapshots.len(), 1);
            assert_eq!(snapshots[0].account_id, leaf);
            assert_eq!(snapshots[0].settled.cr_balance, Decimal::from(25));
        }
    }
}
