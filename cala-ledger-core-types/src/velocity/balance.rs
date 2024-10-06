use rust_decimal::Decimal;

use crate::{balance::BalanceSnapshot, primitives::*};

#[derive(Debug, Clone, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(transparent)]
pub struct Window(serde_json::Value);

impl Window {
    pub fn inner(&self) -> &serde_json::Value {
        &self.0
    }
}

impl From<serde_json::Map<String, serde_json::Value>> for Window {
    fn from(map: serde_json::Map<String, serde_json::Value>) -> Self {
        Window(map.into())
    }
}

impl From<serde_json::Value> for Window {
    fn from(map: serde_json::Value) -> Self {
        Window(map)
    }
}

pub struct VelocityBalance {
    pub control_id: VelocityControlId,
    pub limit_id: VelocityLimitId,
    pub spent: Decimal,
    pub remaining: Decimal,
    pub currency: Currency,
    pub balance: BalanceSnapshot,
}
