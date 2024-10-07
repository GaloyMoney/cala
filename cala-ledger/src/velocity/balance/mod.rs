mod repo;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
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
        controls: HashMap<AccountId, Vec<AccountVelocityControl>>,
    ) -> Result<(), VelocityError> {
        let mut context = super::context::EvalContext::new(transaction);

        let entries_to_enforce =
            Self::determin_entries_to_enforce(&mut context, entries, &controls)?;

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

    fn determin_entries_to_enforce<'a>(
        context: &mut super::context::EvalContext,
        entries: &'a [EntryValues],
        controls: &'a HashMap<AccountId, Vec<AccountVelocityControl>>,
    ) -> Result<
        HashMap<VelocityBalanceKey, Vec<(&'a AccountVelocityLimit, &'a EntryValues)>>,
        VelocityError,
    > {
        let mut entries_to_add: HashMap<
            VelocityBalanceKey,
            Vec<(&AccountVelocityLimit, &EntryValues)>,
        > = HashMap::new();
        for entry in entries {
            let controls = match controls.get(&entry.account_id) {
                Some(control) => control,
                None => continue,
            };
            for control in controls {
                let ctx = context.control_context(entry);
                let control_active = if let Some(condition) = &control.condition {
                    let control_active: bool = condition.try_evaluate(&ctx)?;
                    control_active
                } else {
                    true
                };
                if control_active {
                    for limit in &control.velocity_limits {
                        if let Some(currency) = &limit.currency {
                            if currency != &entry.currency {
                                continue;
                            }
                        }

                        let limit_active = if let Some(condition) = &limit.condition {
                            let limit_active: bool = condition.try_evaluate(&ctx)?;
                            limit_active
                        } else {
                            true
                        };
                        if limit_active {
                            let window = determine_window(&limit.window, &ctx)?;
                            entries_to_add
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

        Ok(entries_to_add)
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
                let ctx = context.control_context(entry);
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

#[cfg(test)]
mod test {
    #[test]
    fn window_determination() {
        use super::*;
        use cala_types::velocity::PartitionKey;
        use cel_interpreter::CelContext;
        use serde_json::json;

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
        let result = determine_window(&keys, &ctx).unwrap();
        let expected = json!({
            "foo": "bar",
            "baz": "qux",
        });
        assert_eq!(Window::from(expected), result);
    }
}
