use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

pub use crate::param::*;
use crate::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VelocityLimitValues {
    pub id: VelocityLimitId,
    pub name: String,
    pub description: String,
    pub window: Vec<PartitionKey>,
    pub condition: Option<CelExpression>,
    pub currency: Option<Currency>,
    pub params: Option<Vec<ParamDefinition>>,
    pub limit: Limit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartitionKey {
    pub alias: String,
    pub value: CelExpression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limit {
    pub timestamp_source: Option<CelExpression>,
    pub balance: Vec<BalanceLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceLimit {
    pub layer: CelExpression,
    pub amount: CelExpression,
    pub enforcement_direction: CelExpression,
}
