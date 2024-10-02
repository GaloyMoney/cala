use rust_decimal::Decimal;

use crate::primitives::{Currency, VelocityControlId, VelocityLimitId};
use cala_types::balance::BalanceSnapshot;

pub struct VelocityBalance {
    control_id: VelocityControlId,
    limit_id: VelocityLimitId,
    spend: Decimal,
    remaining: Decimal,
    currency: Currency,
    balance: BalanceSnapshot,
}
