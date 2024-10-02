mod account_control;
mod balance;
mod context;
mod control;
pub mod error;
mod limit;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use cala_types::{entry::EntryValues, transaction::TransactionValues};

pub use crate::param::Params;
use crate::{atomic_operation::*, outbox::*, primitives::AccountId};

use account_control::*;
pub use control::*;
use error::*;
pub use limit::*;

#[derive(Clone)]
pub struct Velocities {
    outbox: Outbox,
    pool: PgPool,
    limits: VelocityLimitRepo,
    controls: VelocityControlRepo,
    account_controls: AccountControls,
}

impl Velocities {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            limits: VelocityLimitRepo::new(pool),
            controls: VelocityControlRepo::new(pool),
            account_controls: AccountControls::new(pool),
            pool: pool.clone(),
            outbox,
        }
    }

    pub async fn create_limit(
        &self,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let limit = self.create_limit_in_op(&mut op, new_limit).await?;
        op.commit().await?;
        Ok(limit)
    }

    pub async fn create_limit_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        self.limits.create_in_tx(op.tx(), new_limit).await
    }

    pub async fn create_control(
        &self,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let control = self.create_control_in_op(&mut op, new_control).await?;
        op.commit().await?;
        Ok(control)
    }

    pub async fn create_control_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        self.controls.create_in_tx(op.tx(), new_control).await
    }

    pub async fn add_limit_to_control(
        &self,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<(), VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        self.add_limit_to_control_in_op(&mut op, control, limit)
            .await?;
        op.commit().await?;
        Ok(())
    }

    pub async fn add_limit_to_control_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<(), VelocityError> {
        self.limits
            .add_limit_to_control(op.tx(), control, limit)
            .await
    }

    pub async fn attach_control_to_account(
        &self,
        control: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        self.attach_control_to_account_in_op(&mut op, control, account_id, params)
            .await?;
        op.commit().await?;
        Ok(())
    }

    pub async fn attach_control_to_account_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        control_id: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), VelocityError> {
        let control = self.controls.find_by_id(op.tx(), control_id).await?;
        let limits = self.limits.list_for_control(op.tx(), control_id).await?;
        self.account_controls
            .attach_control_in_op(op, control.into_values(), account_id, limits, params)
            .await?;
        Ok(())
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        account_ids: &[AccountId],
    ) -> Result<(), VelocityError> {
        let controls = self
            .account_controls
            .find_for_enforcement(op, account_ids)
            .await?;

        let empty = Vec::new();

        let mut context = context::EvalContext::new(transaction);

        let mut balances_to_load = Vec::new();
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
                            let window = determin_window(&limit.window, &ctx);
                            balances_to_load.push((
                                entry.account_id,
                                control.control_id,
                                limit.limit_id,
                                window,
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn determin_window(
    keys: &[PartitionKey],
    ctx: &cel_interpreter::CelContext,
) -> Result<serde_json::Value, VelocityError> {
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
        let result = determin_window(&keys, &ctx).unwrap();
        let expected = json!({
            "foo": "bar",
            "baz": "qux",
        });
        assert_eq!(expected, result);
    }
}
