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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum VelocityEnforcementAction {
    Reject,
}

impl Default for VelocityEnforcementAction {
    fn default() -> Self {
        VelocityEnforcementAction::Reject
    }
}

impl From<VelocityEnforcementAction> for VelocityEnforcement {
    fn from(action: VelocityEnforcementAction) -> Self {
        VelocityEnforcement { action }
    }
}
