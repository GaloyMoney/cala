use cel_interpreter::CelExpression;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::primitives::{
    AccountId, Currency, DebitOrCredit, Layer, VelocityControlId, VelocityLimitId,
};
use cala_types::velocity::PartitionKey;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityControl {
    pub account_id: AccountId,
    pub control_id: VelocityControlId,
    pub velocity_limits: Vec<AccountVelocityLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountVelocityLimit {
    pub velocity_limit_id: VelocityLimitId,
    pub window: Vec<PartitionKey>,
    pub condition: Option<CelExpression>,
    pub currency: Option<Currency>,
    pub limit: AccountLimit,
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
}
