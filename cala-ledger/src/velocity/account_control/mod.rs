mod repo;
mod value;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{
    account::AccountValuesForContext,
    velocity::{VelocityControlValues, VelocityLimitValues},
};

use crate::{
    ledger_operation::*,
    param::Params,
    primitives::{AccountId, DebitOrCredit, Layer},
};

use super::error::VelocityError;

use repo::*;
pub(super) use value::*;

#[derive(Clone)]
pub struct AccountControls {
    _pool: PgPool,
    repo: AccountControlRepo,
}

impl AccountControls {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: AccountControlRepo::new(pool),
            _pool: pool.clone(),
        }
    }

    pub async fn attach_control_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        created_at: DateTime<Utc>,
        control: &VelocityControlValues,
        account_id: AccountId,
        limits: Vec<VelocityLimitValues>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), VelocityError> {
        let params = params.into();

        let mut velocity_limits = Vec::new();
        for velocity in limits {
            let defs = velocity.params;
            let ctx = params.clone().into_context(defs.as_ref())?;
            let mut limits = Vec::new();
            for limit in velocity.limit.balance {
                let layer: Layer = limit.layer.try_evaluate(&ctx)?;
                let amount: Decimal = limit.amount.try_evaluate(&ctx)?;
                let enforcement_direction: DebitOrCredit =
                    limit.enforcement_direction.try_evaluate(&ctx)?;
                let start = if let Some(start) = limit.start {
                    start.try_evaluate(&ctx)?
                } else {
                    created_at
                };
                let end = if let Some(end) = limit.end {
                    Some(end.try_evaluate(&ctx)?)
                } else {
                    None
                };
                limits.push(AccountBalanceLimit {
                    layer,
                    amount,
                    enforcement_direction,
                    start,
                    end,
                })
            }
            velocity_limits.push(AccountVelocityLimit {
                limit_id: velocity.id,
                window: velocity.window,
                condition: velocity.condition,
                currency: velocity.currency,
                limit: AccountLimit {
                    timestamp_source: velocity.limit.timestamp_source,
                    balance: limits,
                },
            });
        }

        let control = AccountVelocityControl {
            account_id,
            control_id: control.id,
            condition: control.condition.clone(),
            enforcement: control.enforcement.clone(),
            velocity_limits,
        };

        self.repo.create_in_op(db, control).await?;

        Ok(())
    }

    pub async fn find_for_enforcement(
        &self,
        db: &mut LedgerOperation<'_>,
        account_ids: &[AccountId],
    ) -> Result<
        HashMap<AccountId, (AccountValuesForContext, Vec<AccountVelocityControl>)>,
        VelocityError,
    > {
        self.repo.find_for_enforcement(db, account_ids).await
    }
}
