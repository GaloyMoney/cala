use cel_interpreter::CelExpression;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use cala_types::{
    balance::BalanceSnapshot,
    velocity::{PartitionKey, VelocityEnforcement},
};

use crate::{
    primitives::{AccountId, Currency, DebitOrCredit, Layer, VelocityControlId, VelocityLimitId},
    velocity::error::VelocityEnforcementError,
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
    pub fn enforce(&self, _balance: &BalanceSnapshot) -> Result<(), VelocityEnforcementError> {
        // return Err(VelocityEnforcementError::LimitExceeded);
        // let mut spent = Decimal::ZERO;
        // let mut remaining = Decimal::ZERO;
        // for limit in &self.limit.balance {
        //     let layer = limit.layer;
        //     let amount = limit.amount;
        //     let enforcement_direction = limit.enforcement_direction;
        //     let balance = balance.get(layer).unwrap_or(&Decimal::ZERO);
        //     match enforcement_direction {
        //         DebitOrCredit::Debit => {
        //             spent += balance;
        //             remaining += amount - balance;
        //         }
        //         DebitOrCredit::Credit => {
        //             spent += amount - balance;
        //             remaining += balance;
        //         }
        //     }
        // }
        // if spent > Decimal::ZERO {
        //     if spent > self.limit.balance.iter().map(|l| l.amount).sum() {
        //         return Err(VelocityError::LimitExceeded);
        //     }
        // }
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
