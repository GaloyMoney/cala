mod repo;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
    account::AccountValues,
    balance::BalanceSnapshot,
    entry::EntryValues,
    transaction::TransactionValues,
    velocity::{PartitionKey, Window},
};

use crate::{atomic_operation::*, primitives::AccountId};

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
        op: &mut AtomicOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        controls: HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
    ) -> Result<(), VelocityError> {
        let mut context =
            super::context::EvalContext::new(transaction, controls.values().map(|v| &v.0));

        let entries_to_enforce = Self::balances_to_check(&mut context, entries, &controls)?;

        if entries_to_enforce.is_empty() {
            return Ok(());
        }

        let current_balances = self
            .repo
            .find_for_update(op.tx(), entries_to_enforce.keys())
            .await?;

        let new_balances =
            Self::new_snapshots(context, created_at, current_balances, &entries_to_enforce)?;

        self.repo
            .insert_new_snapshots(op.tx(), new_balances)
            .await?;

        Ok(())
    }

    fn balances_to_check<'a>(
        context: &mut super::context::EvalContext,
        entries: &'a [EntryValues],
        controls: &'a HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>,
    ) -> Result<
        HashMap<VelocityBalanceKey, Vec<(&'a AccountVelocityLimit, &'a EntryValues)>>,
        VelocityError,
    > {
        let mut balances_to_check: HashMap<
            VelocityBalanceKey,
            Vec<(&AccountVelocityLimit, &EntryValues)>,
        > = HashMap::new();
        for entry in entries {
            let controls = match controls.get(&entry.account_id) {
                Some(control) => control,
                None => continue,
            };
            for control in controls.1.iter() {
                let ctx = context.context_for_entry(entry);

                if control.needs_enforcement(&ctx)? {
                    for limit in &control.velocity_limits {
                        if let Some(window) = limit.window_for_enforcement(&ctx, entry)? {
                            balances_to_check
                                .entry((
                                    window,
                                    entry.currency,
                                    entry.journal_id,
                                    entry.account_id,
                                    control.control_id,
                                    limit.limit_id,
                                ))
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

            for (limit, entry) in entries {
                let ctx = context.context_for_entry(entry);
                let balance = match (latest_balance.take(), current_balances.remove(key)) {
                    (Some(latest), _) => {
                        new_balances.push(latest.clone());
                        latest
                    }
                    (_, Some(Some(balance))) => balance,
                    (_, Some(None)) => {
                        let new_snapshot =
                            crate::balance::Balances::new_snapshot(time, entry.account_id, entry);
                        limit.enforce(&ctx, time, &new_snapshot)?;
                        latest_balance = Some(new_snapshot);
                        continue;
                    }
                    _ => unreachable!(),
                };
                let new_snapshot = crate::balance::Balances::update_snapshot(time, balance, entry);
                limit.enforce(&ctx, time, &new_snapshot)?;
                new_balances.push(new_snapshot);
            }
            if let Some(latest) = latest_balance.take() {
                new_balances.push(latest)
            }
            res.insert(key, new_balances);
        }
        Ok(res)
    }

    fn determine_window(
        keys: &[PartitionKey],
        ctx: &cel_interpreter::CelContext,
    ) -> Result<Window, VelocityError> {
        let mut map = serde_json::Map::new();
        for key in keys {
            let value: serde_json::Value = key.value.try_evaluate(ctx)?;
            map.insert(key.alias.clone(), value);
        }
        Ok(map.into())
    }
}

#[cfg(test)]
mod test {
    use rust_decimal::Decimal;
    use serde_json::json;

    use cala_types::{account::AccountConfig, velocity::*};
    use cel_interpreter::{CelContext, CelExpression};

    use crate::{primitives::*, velocity::context::EvalContext};

    use super::*;

    #[test]
    fn window_determination() {
        let keys = vec![
            PartitionKey {
                alias: "foo".to_string(),
                value: "'bar'".parse().expect("Failed to parse"),
            },
            PartitionKey {
                alias: "baz".to_string(),
                value: "'qux'".parse().expect("Failed to parse"),
            },
        ];

        let ctx = CelContext::new();
        let result = VelocityBalances::determine_window(&keys, &ctx).unwrap();
        let expected = json!({
            "foo": "bar",
            "baz": "qux",
        });
        assert_eq!(Window::from(expected), result);
    }

    fn transaction(metadata: Option<serde_json::Value>) -> TransactionValues {
        TransactionValues {
            id: TransactionId::new(),
            version: 1,
            journal_id: JournalId::new(),
            tx_template_id: TxTemplateId::new(),
            entry_ids: vec![],
            effective: chrono::Utc::now().date_naive(),
            correlation_id: "correlation_id".to_string(),
            external_id: Some("external_id".to_string()),
            description: None,
            metadata,
        }
    }

    fn account(metadata: Option<serde_json::Value>) -> AccountValues {
        AccountValues {
            id: AccountId::new(),
            version: 1,
            code: "code".to_string(),
            name: "name".to_string(),
            normal_balance_type: DebitOrCredit::Credit,
            status: Status::Active,
            external_id: None,
            description: None,
            metadata,
            config: AccountConfig {
                is_account_set: false,
                eventually_consistent: false,
            },
        }
    }

    fn account_control(
        account_id: AccountId,
        control_condition: Option<CelExpression>,
    ) -> AccountVelocityControl {
        AccountVelocityControl {
            account_id,
            control_id: VelocityControlId::new(),
            enforcement: VelocityEnforcement {
                action: VelocityEnforcementAction::Reject,
            },
            condition: control_condition,
            velocity_limits: vec![AccountVelocityLimit {
                limit_id: VelocityLimitId::new(),
                window: vec![],
                condition: None,
                currency: None,
                limit: AccountLimit {
                    balance: vec![],
                    timestamp_source: None,
                },
            }],
        }
    }

    #[test]
    fn entry_determiniation() {
        let account = account(None);
        let mut context = EvalContext::new(&transaction(None), std::iter::once(&account));
        let control = account_control(account.id, None);
        let controls = std::iter::once((account.id, (account, vec![control]))).collect();

        let result = VelocityBalances::balances_to_check(&mut context, &[], &controls)
            .expect("Failed to determine entries");

        assert!(result.is_empty());
    }
}
