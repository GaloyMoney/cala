use cel_interpreter::CelExpression;
use serde::{Deserialize, Serialize};

use crate::primitives::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VelocityControlValues {
    pub id: VelocityControlId,
    pub name: String,
    pub description: String,
    pub enforcement: VelocityEnforcement,
    pub condition: Option<CelExpression>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct VelocityEnforcement {
    pub action: VelocityEnforcementAction,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VelocityEnforcementAction {
    #[default]
    Reject,
}

impl From<VelocityEnforcementAction> for VelocityEnforcement {
    fn from(action: VelocityEnforcementAction) -> Self {
        VelocityEnforcement { action }
    }
}
