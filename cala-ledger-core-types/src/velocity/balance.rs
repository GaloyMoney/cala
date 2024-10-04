use rust_decimal::Decimal;

use crate::{balance::BalanceSnapshot, primitives::*};

#[derive(Debug, sqlx::Type, PartialEq)]
#[sqlx(transparent)]
pub struct Window(serde_json::Value);

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
