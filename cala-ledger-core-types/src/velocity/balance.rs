use rust_decimal::Decimal;

pub use crate::dimension::Window;
use crate::{balance::BalanceSnapshot, primitives::*};

pub struct VelocityBalance {
    pub control_id: VelocityControlId,
    pub limit_id: VelocityLimitId,
    pub spent: Decimal,
    pub remaining: Decimal,
    pub currency: Currency,
    pub balance: BalanceSnapshot,
}
