mod repo;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
    account::AccountValues, balance::BalanceSnapshot, entry::EntryValues,
    transaction::TransactionValues,
};

use crate::{
    ledger_operation::*,
    primitives::{AccountId, AccountSetId},
};

use super::{account_control::*, error::*};

use repo::*;

#[derive(Clone)]
pub(super) struct VelocityBalances {
    repo: VelocityBalanceRepo,
}

impl VelocityBalances {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: VelocityBalanceRepo::new(pool),
        }
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        controls: HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), VelocityError> {
        let mut context =
            super::context::EvalContext::new(transaction, controls.values().map(|v| &v.0));

        let entries_to_enforce =
            Self::balances_to_check(&mut context, entries, &controls, account_set_mappings)?;

        if entries_to_enforce.is_empty() {
            return Ok(());
        }

        let current_balances = self
            .repo
            .find_for_update(db, entries_to_enforce.keys())
            .await?;

        let new_balances =
            Self::new_snapshots(context, created_at, current_balances, &entries_to_enforce)?;

        self.repo.insert_new_snapshots(db, new_balances).await?;

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn balances_to_check<'a>(
        context: &mut super::context::EvalContext,
        entries: &'a [EntryValues],
        controls: &'a HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<
        HashMap<VelocityBalanceKey, Vec<(&'a AccountVelocityLimit, &'a EntryValues)>>,
        VelocityError,
    > {
        let mut balances_to_check: HashMap<
            VelocityBalanceKey,
            Vec<(&AccountVelocityLimit, &EntryValues)>,
        > = HashMap::new();
        let empty = Vec::new();
        for entry in entries {
            for account_id in account_set_mappings
                .get(&entry.account_id)
                .unwrap_or(&empty)
                .iter()
                .map(AccountId::from)
                .chain(std::iter::once(entry.account_id))
            {
                let Some((_, controls)) = controls.get(&account_id) else {
                    continue;
                };
                let ctx = context.context_for_entry(account_id, entry);

                for control in controls.iter() {
                    if !control.needs_enforcement(&ctx)? {
                        continue;
                    }
                    for limit in &control.velocity_limits {
                        if let Some(window) = limit.window_for_enforcement(&ctx, entry)? {
                            balances_to_check
                                .entry(VelocityBalanceKey {
                                    window,
                                    currency: entry.currency,
                                    journal_id: entry.journal_id,
                                    account_id,
                                    control_id: control.control_id,
                                    limit_id: limit.limit_id,
                                })
                                .or_default()
                                .push((limit, entry));
                        }
                    }
                }
            }
        }

        Ok(balances_to_check)
    }

    fn new_snapshots<'a>(
        mut context: super::context::EvalContext,
        time: DateTime<Utc>,
        mut current_balances: HashMap<VelocityBalanceKey, Option<BalanceSnapshot>>,
        entries_to_add: &'a HashMap<VelocityBalanceKey, Vec<(&AccountVelocityLimit, &EntryValues)>>,
    ) -> Result<HashMap<&'a VelocityBalanceKey, Vec<BalanceSnapshot>>, VelocityError> {
        let mut res = HashMap::new();

        for (key, entries) in entries_to_add.iter() {
            let mut latest_balance: Option<BalanceSnapshot> = None;
            let mut new_balances = Vec::new();

            let current = current_balances
                .remove(key)
                .expect("entries_to_add key missing in current_balances");

            for (limit, entry) in entries {
                let ctx = context.context_for_entry(key.account_id, entry);

                let balance = match latest_balance.or_else(|| current.clone()) {
                    Some(balance) => balance,
                    None => {
                        let new_snapshot =
                            crate::balance::Snapshots::new_snapshot(time, entry.account_id, entry);
                        limit.enforce(&ctx, time, &new_snapshot)?;
                        new_balances.push(new_snapshot.clone());
                        latest_balance = Some(new_snapshot);
                        continue;
                    }
                };

                let new_snapshot = crate::balance::Snapshots::update_snapshot(time, balance, entry);
                limit.enforce(&ctx, time, &new_snapshot)?;
                new_balances.push(new_snapshot.clone());
                latest_balance = Some(new_snapshot);
            }
            res.insert(key, new_balances);
        }
        Ok(res)
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
            account::AccountConfig,
            balance::BalanceAmount,
            velocity::{PartitionKey, Window},
        };

        use crate::{
            primitives::{
                Currency, DebitOrCredit, EntryId, JournalId, Layer, Status, TransactionId,
                TxTemplateId, VelocityControlId, VelocityLimitId,
            },
            velocity::{
                account_control::{AccountBalanceLimit, AccountLimit, AccountVelocityLimit},
                context::EvalContext,
            },
        };

        fn create_test_entry(
            units: Decimal,
            direction: DebitOrCredit,
            layer: Layer,
            currency: &str,
        ) -> EntryValues {
            EntryValues {
                id: EntryId::new(),
                version: 1,
                transaction_id: TransactionId::new(),
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
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

        fn create_test_limit(
            limit_id: VelocityLimitId,
            amount: Decimal,
            enforcement_direction: DebitOrCredit,
            layer: Layer,
        ) -> AccountVelocityLimit {
            AccountVelocityLimit {
                limit_id,
                window: vec![PartitionKey {
                    alias: "test".to_string(),
                    value: "\"test_window\"".parse().unwrap(),
                }],
                condition: None,
                currency: None,
                limit: AccountLimit {
                    timestamp_source: None,
                    balance: vec![AccountBalanceLimit {
                        layer,
                        amount,
                        enforcement_direction,
                        start: Utc::now() - chrono::Duration::seconds(1), // Start 1 second ago to ensure it's active
                        end: None,
                    }],
                },
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

        fn create_test_key() -> VelocityBalanceKey {
            VelocityBalanceKey {
                window: Window::from(serde_json::json!({"test": "test_window"})),
                currency: "USD".parse().unwrap(),
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
                control_id: VelocityControlId::new(),
                limit_id: VelocityLimitId::new(),
            }
        }

        fn create_test_transaction() -> TransactionValues {
            TransactionValues {
                id: TransactionId::new(),
                version: 1,
                created_at: chrono::Utc::now(),
                modified_at: chrono::Utc::now(),
                journal_id: JournalId::new(),
                tx_template_id: TxTemplateId::new(),
                entry_ids: vec![],
                effective: chrono::Utc::now().date_naive(),
                correlation_id: "test-correlation".to_string(),
                external_id: Some("test-external".to_string()),
                description: None,
                void_of: None,
                voided_by: None,
                metadata: None,
            }
        }

        fn create_test_account(id: AccountId) -> AccountValues {
            AccountValues {
                id,
                version: 1,
                code: "TEST_ACCOUNT".to_string(),
                name: "Test Account".to_string(),
                external_id: None,
                normal_balance_type: DebitOrCredit::Debit,
                status: Status::Active,
                description: None,
                config: AccountConfig::default(),
                metadata: None,
            }
        }

        #[test]
        fn new_snapshots_empty_entries_returns_empty() {
            let transaction = create_test_transaction();
            let accounts = vec![];
            let context = EvalContext::new(&transaction, accounts.iter());
            let time = Utc::now();
            let current_balances = HashMap::new();
            let entries_to_add = HashMap::new();

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert!(result.is_empty());
        }

        #[test]
        fn new_snapshots_single_entry_no_previous_balance() {
            let time = Utc::now();
            let key = create_test_key();
            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(1000),
                DebitOrCredit::Debit,
                Layer::Settled,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 1);

            let snapshot = &snapshots[0];
            assert_eq!(snapshot.version, 1);
            assert_eq!(snapshot.settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshot.settled.cr_balance, Decimal::ZERO);
        }

        #[test]
        fn new_snapshots_single_entry_with_existing_balance() {
            let time = Utc::now();
            let key = create_test_key();
            let mut entry = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(1000),
                DebitOrCredit::Credit,
                Layer::Pending,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let existing_balance =
                create_test_balance_snapshot(key.account_id, key.journal_id, key.currency, 5);
            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), Some(existing_balance));

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 1); // Only the updated snapshot

            // The snapshot is updated with new entry
            let snapshot = &snapshots[0];
            assert_eq!(snapshot.version, 6); // Previous version was 5
            assert_eq!(snapshot.pending.cr_balance, Decimal::from(50));
            assert_eq!(snapshot.pending.dr_balance, Decimal::ZERO);
        }

        #[test]
        fn new_snapshots_multiple_entries_same_key() {
            let time = Utc::now();
            let key = create_test_key();
            let mut entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            let mut entry2 = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Credit,
                Layer::Settled,
                "USD",
            );
            // Ensure both entries use the same account ID as the key
            entry1.account_id = key.account_id;
            entry2.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(1000),
                DebitOrCredit::Debit,
                Layer::Settled,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry1), (&limit, &entry2)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 2); // Two snapshots for two entries

            // First snapshot
            assert_eq!(snapshots[0].version, 1);
            assert_eq!(snapshots[0].settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshots[0].settled.cr_balance, Decimal::ZERO);

            // Second snapshot builds on first
            assert_eq!(snapshots[1].version, 2);
            assert_eq!(snapshots[1].settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshots[1].settled.cr_balance, Decimal::from(50));
        }

        #[test]
        fn new_snapshots_multiple_keys() {
            let time = Utc::now();

            let key1 = create_test_key();
            let key2 = VelocityBalanceKey {
                window: Window::from(serde_json::json!({"test": "test_window2"})),
                currency: "EUR".parse().unwrap(),
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
                control_id: VelocityControlId::new(),
                limit_id: VelocityLimitId::new(),
            };

            let mut entry1 = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            let mut entry2 = create_test_entry(
                Decimal::from(200),
                DebitOrCredit::Credit,
                Layer::Pending,
                "EUR",
            );
            // Ensure entries use the same account_id as their respective keys
            entry1.account_id = key1.account_id;
            entry2.account_id = key2.account_id;
            let limit1 = create_test_limit(
                key1.limit_id,
                Decimal::from(1000),
                DebitOrCredit::Debit,
                Layer::Settled,
            );
            let limit2 = create_test_limit(
                key2.limit_id,
                Decimal::from(2000),
                DebitOrCredit::Credit,
                Layer::Pending,
            );

            let transaction = create_test_transaction();
            let account1 = create_test_account(key1.account_id);
            let account2 = create_test_account(key2.account_id);
            let context = EvalContext::new(&transaction, [&account1, &account2].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key1.clone(), None);
            current_balances.insert(key2.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key1.clone(), vec![(&limit1, &entry1)]);
            entries_to_add.insert(key2.clone(), vec![(&limit2, &entry2)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 2);
            assert!(result.contains_key(&key1));
            assert!(result.contains_key(&key2));
        }

        #[test]
        fn new_snapshots_enforcement_failure() {
            let time = Utc::now();
            let key = create_test_key();
            // Entry tries to debit 500, but limit only allows 100
            let mut entry = create_test_entry(
                Decimal::from(500),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add);

            match result {
                Ok(_) => panic!("Expected enforcement failure, but got success"),
                Err(err) => match err {
                    VelocityError::Enforcement(e) => {
                        assert_eq!(e.requested, Decimal::from(500));
                        assert_eq!(e.limit, Decimal::from(100));
                    }
                    _ => panic!("Expected Enforcement error, got: {:?}", err),
                },
            }
        }

        #[test]
        fn new_snapshots_layer_enforcement() {
            let time = Utc::now();
            let key = create_test_key();

            // Entry on Settled layer, limit enforces Pending layer (which includes Settled)
            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(150),
                DebitOrCredit::Debit,
                Layer::Pending,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 1);
            assert_eq!(snapshots[0].settled.dr_balance, Decimal::from(100));
        }

        #[test]
        #[should_panic(expected = "entries_to_add key missing in current_balances")]
        fn new_snapshots_missing_key_in_current_balances() {
            let time = Utc::now();
            let key = create_test_key();
            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(1000),
                DebitOrCredit::Debit,
                Layer::Settled,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            // Don't add the key to current_balances
            let current_balances = HashMap::new();

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let _ =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add);
        }

        #[test]
        fn new_snapshots_multiple_limits_per_entry() {
            let time = Utc::now();
            let key = create_test_key();
            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit1 = create_test_limit(
                VelocityLimitId::new(),
                Decimal::from(200),
                DebitOrCredit::Debit,
                Layer::Settled,
            );
            let limit2 = create_test_limit(
                VelocityLimitId::new(),
                Decimal::from(300),
                DebitOrCredit::Debit,
                Layer::Settled,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            // Same entry with two different limits
            entries_to_add.insert(key.clone(), vec![(&limit1, &entry), (&limit2, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 2); // One snapshot per limit-entry pair

            // Both snapshots should have the same balance values
            assert_eq!(snapshots[0].settled.dr_balance, Decimal::from(100));
            assert_eq!(snapshots[1].settled.dr_balance, Decimal::from(200)); // Accumulated
        }

        #[test]
        fn new_snapshots_credit_enforcement() {
            let time = Utc::now();
            let key = create_test_key();

            // Test credit enforcement
            let mut entry = create_test_entry(
                Decimal::from(300),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(500),
                DebitOrCredit::Credit,
                Layer::Pending,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots[0].pending.cr_balance, Decimal::from(300));
        }

        #[test]
        fn new_snapshots_encumbrance_layer() {
            let time = Utc::now();
            let key = create_test_key();

            let mut entry = create_test_entry(
                Decimal::from(75),
                DebitOrCredit::Debit,
                Layer::Encumbrance,
                "USD",
            );
            // Ensure entry uses the same account_id as the key
            entry.account_id = key.account_id;
            let limit = create_test_limit(
                key.limit_id,
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Encumbrance,
            );

            let transaction = create_test_transaction();
            let account = create_test_account(key.account_id);
            let context = EvalContext::new(&transaction, [&account].into_iter());

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let result =
                VelocityBalances::new_snapshots(context, time, current_balances, &entries_to_add)
                    .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots[0].encumbrance.dr_balance, Decimal::from(75));
        }
    }
}
