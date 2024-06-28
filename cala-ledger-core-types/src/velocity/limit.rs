use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

use crate::{param::*, primitives::*};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VelocityLimitValues {
    pub id: VelocityLimitId,
    pub name: String,
    pub description: String,
    pub window: Vec<PartitionKeyInput>,
    pub condition: Option<CelExpression>,
    pub currency: Option<Currency>,
    pub params: Option<Vec<ParamDefinition>>,
    pub limit: LimitInput,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartitionKeyInput {
    pub alias: String,
    pub value: CelExpression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LimitInput {
    pub timestamp_source: Option<CelExpression>,
    pub balance: Vec<BalanceLimitInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceLimitInput {
    pub layer: CelExpression,
    pub amount: CelExpression,
    pub enforcement_direction: CelExpression,
}
