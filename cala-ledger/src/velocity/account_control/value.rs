use cel_interpreter::{CelContext, CelExpression};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use cala_types::{
    balance::BalanceSnapshot,
    velocity::{PartitionKey, VelocityEnforcement},
};

use crate::{
    primitives::{AccountId, Currency, DebitOrCredit, Layer, VelocityControlId, VelocityLimitId},
    velocity::error::*,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityControl {
    pub account_id: AccountId,
    pub control_id: VelocityControlId,
    pub enforcement: VelocityEnforcement,
    pub condition: Option<CelExpression>,
    pub velocity_limits: Vec<AccountVelocityLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityLimit {
    pub limit_id: VelocityLimitId,
    pub window: Vec<PartitionKey>,
    pub condition: Option<CelExpression>,
    pub currency: Option<Currency>,
    pub limit: AccountLimit,
}

impl AccountVelocityLimit {
    pub fn enforce(
        &self,
        ctx: &CelContext,
        time: DateTime<Utc>,
        snapshot: &BalanceSnapshot,
    ) -> Result<(), VelocityError> {
        if let Some(currency) = &self.currency {
            if currency != &snapshot.currency {
                return Ok(());
            }
        }
        let time = if let Some(source) = &self.limit.timestamp_source {
            source.try_evaluate(ctx)?
        } else {
            time
        };
        for limit in self.limit.balance.iter() {
            if limit.start > time {
                continue;
            }
            if let Some(end) = limit.end {
                if end <= time {
                    continue;
                }
            }
            let balance =
                crate::balance::BalanceWithDirection::new(limit.enforcement_direction, snapshot);
            let requested = balance.available(limit.layer);

            if requested > limit.amount {
                return Err(LimitExceededError {
                    account_id: snapshot.account_id,
                    currency: snapshot.currency.code().to_string(),
                    limit_id: self.limit_id,
                    layer: limit.layer,
                    limit: limit.amount,
                    requested,
                }
                .into());
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountLimit {
    pub timestamp_source: Option<CelExpression>,
    pub balance: Vec<AccountBalanceLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountBalanceLimit {
    pub layer: Layer,
    pub amount: Decimal,
    pub enforcement_direction: DebitOrCredit,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}
