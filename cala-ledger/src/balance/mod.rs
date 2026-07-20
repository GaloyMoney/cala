//! # EC set balance maintenance
//!
//! Non-EC account-set balances are maintained **inline** by posters
//! (`update_balances_in_op`), synchronously in the posting transaction.
//! Eventually-consistent (EC) set balances are excluded from that path
//! (`find_for_update` filters `eventually_consistent = FALSE`) and are
//! instead maintained **asynchronously** by the streaming rollup job
//! ([`crate::ec_rollup`]), which folds each committed transaction's leaf
//! deltas into its ancestor EC sets. That single, ordered, `spawn_unique`
//! writer is the only maintainer of EC-set balances.
//!
//! Posters take a shared advisory lock (`EC_SET_LOCK_CLASS`) on every
//! account they touch — leaves and ancestors alike. Its load-bearing role
//! is the member side of the membership guard
//! (`member_has_balance_history_in_op`): adding or removing an EC-set
//! member takes an EXCLUSIVE lock on that member, so a concurrent poster's
//! SHARED lock on the same member blocks until the guard's history check
//! has committed. That keeps a member from ever joining or leaving a set
//! while it has balance history, which is what makes EC sets
//! incremental-from-birth for the streaming rollup.

mod account_balance;
mod cursor;
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
pub use cursor::*;
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
            effective: EffectiveBalances::new(pool, publisher),
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

    #[instrument(name = "cala_ledger.balance.list_for_account", skip(self))]
    pub async fn list_for_account(
        &self,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account(journal_id, account_id.into(), args)
            .await
    }

    #[instrument(name = "cala_ledger.balance.list_for_accounts", skip(self))]
    pub async fn list_for_accounts(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts(journal_id, account_ids, args)
            .await
    }

    #[instrument(name = "cala_ledger.balance.find_all_in_op", skip(self, op))]
    pub async fn find_all_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        ids: &[BalanceId],
    ) -> Result<HashMap<BalanceId, AccountBalance>, BalanceError> {
        self.repo.find_all_in_op(op, ids).await
    }

    #[instrument(name = "cala_ledger.balance.list_for_account_in_op", skip(self, op))]
    pub async fn list_for_account_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_id: impl Into<AccountId> + std::fmt::Debug,
        args: es_entity::PaginatedQueryArgs<AccountBalanceByCurrencyCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceByCurrencyCursor>,
        BalanceError,
    > {
        self.repo
            .list_for_account_in_op(op, journal_id, account_id.into(), args)
            .await
    }

    #[instrument(name = "cala_ledger.balance.list_for_accounts_in_op", skip(self, op))]
    pub async fn list_for_accounts_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
        args: es_entity::PaginatedQueryArgs<AccountBalanceCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountBalance, AccountBalanceCursor>, BalanceError>
    {
        self.repo
            .list_for_accounts_in_op(op, journal_id, account_ids, args)
            .await
    }

    #[instrument(
        name = "cala_ledger.balance.update_balances_in_op",
        skip(self, op, entries, account_set_mappings),
        fields(journal_id = %journal_id, entries_count = entries.len()),
        err(level = "warn")
    )]
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

    /// Streaming EC rollup for a single committed transaction.
    ///
    /// Mirror of [`Self::update_balances_in_op`] but for the ancestor
    /// **eventually-consistent** account sets — the ones the inline poster
    /// path deliberately excludes. Given the transaction's `entries`, fold
    /// their deltas into every EC ancestor set (settled + effective), under
    /// the shared EC-set advisory lock. Caller drives this per transaction
    /// from the outbox and owns the batch/commit/cursor
    /// (see [`crate::ec_rollup`]).
    #[instrument(
        name = "cala_ledger.balance.apply_ec_rollup_in_op",
        skip(self, op, entries),
        fields(journal_id = %journal_id, entries_count = entries.len()),
        err(level = "warn")
    )]
    pub(crate) async fn apply_ec_rollup_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entries: Vec<EntryValues>,
        effective: NaiveDate,
        created_at: DateTime<Utc>,
    ) -> Result<(), BalanceError> {
        if entries.is_empty() {
            return Ok(());
        }

        let member_account_ids: Vec<AccountId> = entries
            .iter()
            .map(|e| e.account_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let ec_mappings = self
            .repo
            .fetch_ec_set_mappings(op, journal_id, &member_account_ids)
            .await?;
        if ec_mappings.is_empty() {
            return Ok(());
        }

        // Distinct (EC set-account, currency) pairs touched by this tx.
        let empty = Vec::new();
        let mut involved: BTreeSet<(AccountId, Currency)> = BTreeSet::new();
        for entry in entries.iter() {
            for set_id in ec_mappings.get(&entry.account_id).unwrap_or(&empty) {
                involved.insert((AccountId::from(set_id), entry.currency));
            }
        }
        if involved.is_empty() {
            return Ok(());
        }
        let (account_ids, currencies): (Vec<AccountId>, Vec<&str>) =
            involved.into_iter().map(|(a, c)| (a, c.code())).unzip();

        let current_balances = self
            .repo
            .find_ec_balances_for_update(op, journal_id, &(account_ids.clone(), currencies.clone()))
            .await?;

        let new_balances =
            Self::ec_set_snapshots(created_at, current_balances, &entries, &ec_mappings);
        if !new_balances.is_empty() {
            self.repo
                .insert_new_snapshots(op, journal_id, new_balances)
                .await?;
        }

        let journal = self.journals.find(journal_id).await?;
        if journal.insert_effective_balances() {
            self.effective
                .apply_ec_rollup_in_op(
                    op,
                    journal_id,
                    entries,
                    effective,
                    created_at,
                    ec_mappings,
                    (account_ids, currencies),
                )
                .await?;
        }

        Ok(())
    }

    /// Return `true` iff `member_id` has any row in
    /// `cala_balance_history` for `journal_id`, under the lock prelude
    /// described on `BalanceRepo::member_has_balance_history_in_op`.
    #[instrument(
        name = "cala_ledger.balance.member_has_balance_history_in_op",
        skip(self, op),
        fields(
            journal_id = %journal_id,
            parent_account_id = %parent_account_id,
            member_id = %member_id,
        ),
        err(level = "warn")
    )]
    pub(crate) async fn member_has_balance_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        parent_account_id: AccountId,
        member_id: AccountId,
    ) -> Result<bool, BalanceError> {
        self.repo
            .member_has_balance_history_in_op(op, journal_id, parent_account_id, member_id)
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

    /// Like [`Self::new_snapshots`] but fans each entry **only** into its
    /// EC ancestor sets (never the leaf account itself, which the inline
    /// poster path already maintains). Chains repeated writes to the same
    /// set within the batch so versions increment correctly.
    #[instrument(name = "cala_ledger.balances.ec_set_snapshots", skip_all)]
    fn ec_set_snapshots(
        time: DateTime<Utc>,
        mut current_balances: HashMap<(AccountId, Currency), Option<BalanceSnapshot>>,
        entries: &[EntryValues],
        ec_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Vec<BalanceSnapshot> {
        let mut latest_balances: HashMap<(AccountId, &Currency), BalanceSnapshot> = HashMap::new();
        let mut new_balances = Vec::new();
        let empty = Vec::new();
        for entry in entries.iter() {
            for account_id in ec_mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
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

    mod ec_set_snapshots {
        use super::*;

        use chrono::Utc;
        use rust_decimal::Decimal;
        use std::collections::HashMap;

        use cala_types::{
            entry::EntryValues,
            primitives::{DebitOrCredit, Layer},
        };

        use crate::primitives::{AccountSetId, Currency, EntryId, JournalId, TransactionId};

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

            let snapshots = Balances::ec_set_snapshots(Utc::now(), current, &entries, &ec_mappings);

            // Only the EC set is written — never the leaf accounts.
            assert!(snapshots.iter().all(|s| s.account_id == set_account));
            // The final (highest-version) snapshot reflects both credits.
            let final_snapshot = snapshots.iter().max_by_key(|s| s.version).unwrap();
            assert_eq!(final_snapshot.settled.cr_balance, Decimal::from(150));
            assert_eq!(final_snapshot.version, 2);
        }

        #[test]
        fn skips_members_without_ec_ancestors() {
            let entries = vec![credit_entry(Decimal::from(10), AccountId::new())];
            let ec_mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
            let current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> = HashMap::new();

            let snapshots = Balances::ec_set_snapshots(Utc::now(), current, &entries, &ec_mappings);
            assert!(snapshots.is_empty());
        }
    }
}
