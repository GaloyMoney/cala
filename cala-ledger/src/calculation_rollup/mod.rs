//! Streaming maintenance of calculation effective balance series: the
//! always-on [tip](tip) and the one-off [backfills](backfill), landing the
//! same [batch](batch) under different [`RollupScope`](crate::calculation::RollupScope)s.
mod backfill;
mod batch;
mod tip;

pub(crate) use backfill::{
    register_calculation_backfill, BackfillSubscriptions, BackfillUnit, CalculationBackfillId,
    BACKFILL_WAKE_KEY,
};
pub(crate) use tip::register_calculation_rollup;
pub use tip::CalculationRollupStatus;
