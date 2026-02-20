mod repo;

use chrono::{DateTime, Utc};
use es_entity::clock::ClockHandle;
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
    balance::BalanceSnapshot, entry::EntryValues, transaction::TransactionValues,
    velocity::VelocityContextAccountValues,
};

use cala_types::primitives::{AccountId, AccountSetId};

use super::{account_control::*, error::*};

use repo::*;

#[derive(Clone)]
pub(super) struct VelocityBalances {
    repo: VelocityBalanceRepo,
    clock: ClockHandle,
}

impl VelocityBalances {
    pub fn new(pool: &PgPool, clock: &ClockHandle) -> Self {
        Self {
            repo: VelocityBalanceRepo::new(pool),
            clock: clock.clone(),
        }
    }

    pub(crate) async fn update_balances_with_limit_enforcement_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        controls: HashMap<AccountId, (VelocityContextAccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), VelocityError> {
        let mut context = super::context::EvalContext::new(
            self.clock.clone(),
            transaction,
            controls.values().map(|v| &v.0),
        );

        let entries_to_enforce =
            Self::balances_to_check(&mut context, entries, &controls, account_set_mappings)?;

        if entries_to_enforce.is_empty() {
            return Ok(());
        }

        let current_balances = self
            .repo
            .find_for_update(db, entries_to_enforce.keys())
            .await?;

        let new_balances = Self::new_snapshots_with_limit_enforcement(
            context,
            created_at,
            current_balances,
            &entries_to_enforce,
        )?;

        self.repo.insert_new_snapshots(db, new_balances).await?;

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn balances_to_check<'a>(
        context: &mut super::context::EvalContext,
        entries: &'a [EntryValues],
        controls: &'a HashMap<
            AccountId,
            (VelocityContextAccountValues, Vec<AccountVelocityControl>),
        >,
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

    fn new_snapshots_with_limit_enforcement<'a>(
        mut context: super::context::EvalContext,
        time: DateTime<Utc>,
        mut current_balances: HashMap<VelocityBalanceKey, Option<BalanceSnapshot>>,
        entries_to_add: &'a HashMap<VelocityBalanceKey, Vec<(&AccountVelocityLimit, &EntryValues)>>,
    ) -> Result<HashMap<&'a VelocityBalanceKey, Vec<BalanceSnapshot>>, VelocityError> {
        let mut res = HashMap::new();

        for (key, entries) in entries_to_add.iter() {
            let mut latest_balance = current_balances
                .remove(key)
                .expect("entries_to_add key missing in current_balances");

            let mut new_balances = Vec::new();

            for (limit, entry) in entries {
                let new_balance = match latest_balance.take() {
                    Some(balance) => {
                        cala_types::balance::Snapshots::update_snapshot(time, balance, entry)
                    }
                    None => {
                        cala_types::balance::Snapshots::new_snapshot(time, entry.account_id, entry)
                    }
                };

                let ctx = context.context_for_entry(key.account_id, entry);
                limit.enforce(&ctx, time, &new_balance)?;

                new_balances.push(new_balance.clone());
                latest_balance = Some(new_balance);
            }

            res.insert(key, new_balances);
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod new_snapshots_with_limit_enforcement {
        use super::*;

        use chrono::Utc;
        use es_entity::clock::Clock;
        use rust_decimal::Decimal;
        use std::collections::HashMap;

        use cala_types::{balance::BalanceAmount, velocity::Window};

        use cala_types::primitives::{
            Currency, DebitOrCredit, EntryId, JournalId, Layer, TransactionId, TxTemplateId,
            VelocityControlId, VelocityLimitId,
        };

        use crate::{
            account_control::{AccountBalanceLimit, AccountLimit, AccountVelocityLimit},
            context::EvalContext,
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

        fn dummy_test_limit() -> AccountVelocityLimit {
            AccountVelocityLimit {
                limit_id: VelocityLimitId::new(),
                window: Default::default(),
                condition: None,
                currency: None,
                limit: AccountLimit {
                    timestamp_source: None,
                    balance: Default::default(),
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

        fn create_test_account_values(id: AccountId) -> VelocityContextAccountValues {
            VelocityContextAccountValues {
                id,
                name: "Test Account".to_string(),
                external_id: None,
                normal_balance_type: DebitOrCredit::Debit,
                metadata: None,
            }
        }

        #[test]
        fn new_snapshots_from_single_entry_with_existing_balance() {
            let key = create_test_key();
            let limit = dummy_test_limit();

            let transaction = create_test_transaction();
            let account = create_test_account_values(key.account_id);
            let context = EvalContext::new(
                Clock::handle().clone(),
                &transaction,
                [&account].into_iter(),
            );

            let mut entry = create_test_entry(
                Decimal::from(50),
                DebitOrCredit::Credit,
                Layer::Pending,
                "USD",
            );
            entry.account_id = key.account_id;

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let version = 5;
            let existing_balance =
                create_test_balance_snapshot(key.account_id, key.journal_id, key.currency, version);
            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), Some(existing_balance));

            let result = VelocityBalances::new_snapshots_with_limit_enforcement(
                context,
                Utc::now(),
                current_balances,
                &entries_to_add,
            )
            .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 1);
            let snapshot = &snapshots[0];
            assert_eq!(snapshot.version, version + 1);
        }

        #[test]
        fn new_snapshots_from_single_entry_no_previous_balance() {
            let key = create_test_key();
            let limit = dummy_test_limit();

            let transaction = create_test_transaction();
            let account = create_test_account_values(key.account_id);
            let context = EvalContext::new(
                Clock::handle().clone(),
                &transaction,
                [&account].into_iter(),
            );

            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            entry.account_id = key.account_id;

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let result = VelocityBalances::new_snapshots_with_limit_enforcement(
                context,
                Utc::now(),
                current_balances,
                &entries_to_add,
            )
            .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 1);
            let snapshot = &snapshots[0];
            assert_eq!(snapshot.version, 1);
        }

        #[test]
        fn new_snapshots_can_update_from_multiple_entries() {
            let key = create_test_key();
            let limit = dummy_test_limit();

            let transaction = create_test_transaction();
            let account = create_test_account_values(key.account_id);
            let context = EvalContext::new(
                Clock::handle().clone(),
                &transaction,
                [&account].into_iter(),
            );

            let initial_debit = Decimal::from(100);
            let initial_credit = Decimal::from(25);
            let version = 3;
            let mut current_balance =
                create_test_balance_snapshot(key.account_id, key.journal_id, key.currency, version);
            current_balance.settled.dr_balance = initial_debit;
            current_balance.settled.cr_balance = initial_credit;

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), Some(current_balance));

            // First entry will create a latest balance
            let entry1_debit = Decimal::from(50);
            let mut entry1 =
                create_test_entry(entry1_debit, DebitOrCredit::Debit, Layer::Settled, "USD");
            entry1.account_id = key.account_id;

            // Second entry should use the latest balance, not the current
            let entry2_credit = Decimal::from(30);
            let mut entry2 =
                create_test_entry(entry2_credit, DebitOrCredit::Credit, Layer::Settled, "USD");
            entry2.account_id = key.account_id;

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry1), (&limit, &entry2)]);

            let result = VelocityBalances::new_snapshots_with_limit_enforcement(
                context,
                Utc::now(),
                current_balances,
                &entries_to_add,
            )
            .unwrap();

            assert_eq!(result.len(), 1);
            let snapshots = result.get(&key).unwrap();
            assert_eq!(snapshots.len(), 2);

            assert_eq!(snapshots[0].version, version + 1);
            assert_eq!(
                snapshots[0].settled.dr_balance,
                initial_debit + entry1_debit
            );

            assert_eq!(snapshots[1].version, version + 2);
            assert_eq!(
                snapshots[1].settled.cr_balance,
                initial_credit + entry2_credit
            );
        }

        #[test]
        #[should_panic(expected = "entries_to_add key missing in current_balances")]
        fn new_snapshots_fails_for_missing_key_in_current_balances() {
            let key = create_test_key();
            let limit = dummy_test_limit();

            let transaction = create_test_transaction();
            let account = create_test_account_values(key.account_id);
            let context = EvalContext::new(
                Clock::handle().clone(),
                &transaction,
                [&account].into_iter(),
            );

            let mut entry = create_test_entry(
                Decimal::from(100),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            entry.account_id = key.account_id;

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let current_balances = HashMap::new();

            let _ = VelocityBalances::new_snapshots_with_limit_enforcement(
                context,
                Utc::now(),
                current_balances,
                &entries_to_add,
            );
        }

        #[test]
        fn new_snapshots_checks_limits() {
            let key = create_test_key();

            let transaction = create_test_transaction();
            let account = create_test_account_values(key.account_id);
            let context = EvalContext::new(
                Clock::handle().clone(),
                &transaction,
                [&account].into_iter(),
            );

            let limit = AccountVelocityLimit {
                limit_id: key.limit_id,
                window: Default::default(),
                condition: None,
                currency: None,
                limit: AccountLimit {
                    timestamp_source: None,
                    balance: vec![AccountBalanceLimit {
                        layer: Layer::Settled,
                        amount: Decimal::from(100),
                        enforcement_direction: DebitOrCredit::Debit,
                        start: Utc::now() - chrono::Duration::seconds(1), // Start 1 second ago to ensure it's active
                        end: None,
                    }],
                },
            };

            let mut entry = create_test_entry(
                Decimal::from(500),
                DebitOrCredit::Debit,
                Layer::Settled,
                "USD",
            );
            entry.account_id = key.account_id;

            let mut entries_to_add = HashMap::new();
            entries_to_add.insert(key.clone(), vec![(&limit, &entry)]);

            let mut current_balances = HashMap::new();
            current_balances.insert(key.clone(), None);

            let result = VelocityBalances::new_snapshots_with_limit_enforcement(
                context,
                Utc::now(),
                current_balances,
                &entries_to_add,
            );
            assert!(matches!(result, Err(VelocityError::Enforcement(_))));
        }
    }
}
