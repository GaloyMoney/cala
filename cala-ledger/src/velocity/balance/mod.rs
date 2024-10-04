mod repo;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
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
        _created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        controls: HashMap<AccountId, Vec<AccountVelocityControl>>,
    ) -> Result<(), VelocityError> {
        let empty = Vec::new();

        let mut context = super::context::EvalContext::new(transaction);

        let mut entries_to_add: HashMap<
            VelocityBalanceKey,
            Vec<(&AccountVelocityLimit, &EntryValues)>,
        > = HashMap::new();
        for entry in entries {
            for control in controls.get(&entry.account_id).unwrap_or(&empty) {
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

        if entries_to_add.is_empty() {
            return Ok(());
        }

        let _current_balances = self
            .repo
            .find_for_update(op.tx(), entries_to_add.keys())
            .await?;

        Ok(())
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
