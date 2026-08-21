mod repo;
mod value;

use es_entity::clock::ClockHandle;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::instrument;

use cala_types::velocity::{VelocityControlValues, VelocityLimitValues};

use crate::{
    param::Params,
    primitives::{AccountId, DebitOrCredit, Layer},
};

use super::error::VelocityError;

use repo::*;
pub(crate) use value::*;

#[derive(Clone)]
pub struct AccountControls {
    _pool: PgPool,
    repo: AccountControlRepo,
    clock: ClockHandle,
}

impl AccountControls {
    pub fn new(pool: &PgPool, clock: &ClockHandle) -> Self {
        Self {
            repo: AccountControlRepo::new(pool),
            _pool: pool.clone(),
            clock: clock.clone(),
        }
    }

    pub async fn attach_control_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control: &VelocityControlValues,
        account_id: AccountId,
        limits: Vec<VelocityLimitValues>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), VelocityError> {
        let velocity_limits = Self::evaluate_velocity_limits(&self.clock, limits, params.into())?;

        let control = AccountVelocityControl {
            account_id,
            control_id: control.id,
            condition: control.condition.clone(),
            enforcement: control.enforcement.clone(),
            velocity_limits,
        };

        self.repo.create_in_op(db, control).await?;

        Ok(())
    }

    /// Batched counterpart of [`Self::attach_control_in_op`]: attaches the
    /// same `control` (with the same `params`) to every account in
    /// `account_ids` in one round trip.
    ///
    /// `params` is shared across the whole batch (a single
    /// `impl Into<Params>`, not `Vec<Params>`) — every account is attached
    /// to the same control under the same evaluated condition and limits.
    /// Accounts that need different params attach in separate calls.
    ///
    /// Because `params` (and therefore every evaluated `condition` /
    /// `AccountVelocityLimit`) is identical for every account in the
    /// batch, the CEL evaluation that builds `velocity_limits` runs
    /// **once** for the whole batch and is cloned per account — the
    /// per-row difference is only `account_id`.
    #[instrument(
        level = "debug",
        name = "account_control.attach_control_to_accounts_in_op",
        skip(self, db, limits, params),
        fields(control_id = %control.id, account_count = account_ids.len()),
        err(level = "warn")
    )]
    pub async fn attach_control_to_accounts_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control: &VelocityControlValues,
        account_ids: &[AccountId],
        limits: Vec<VelocityLimitValues>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<(), VelocityError> {
        if account_ids.is_empty() {
            return Ok(());
        }

        let velocity_limits = Self::evaluate_velocity_limits(&self.clock, limits, params.into())?;

        let controls = account_ids
            .iter()
            .map(|&account_id| AccountVelocityControl {
                account_id,
                control_id: control.id,
                condition: control.condition.clone(),
                enforcement: control.enforcement.clone(),
                velocity_limits: velocity_limits.clone(),
            })
            .collect();

        self.repo.create_all_in_op(db, controls).await?;

        Ok(())
    }

    fn evaluate_velocity_limits(
        clock: &ClockHandle,
        limits: Vec<VelocityLimitValues>,
        params: Params,
    ) -> Result<Vec<AccountVelocityLimit>, VelocityError> {
        let mut velocity_limits = Vec::new();
        for velocity in limits {
            let defs = velocity.params;
            let ctx = params.clone().into_context(clock, defs.as_ref())?;
            let mut limits = Vec::new();
            for limit in velocity.limit.balance {
                let layer: Layer = limit.layer.try_evaluate(&ctx)?;
                let amount: Decimal = limit.amount.try_evaluate(&ctx)?;
                let enforcement_direction: DebitOrCredit =
                    limit.enforcement_direction.try_evaluate(&ctx)?;
                let start = limit.start.try_evaluate(&ctx)?;
                let end = if let Some(end) = limit.end {
                    Some(end.try_evaluate(&ctx)?)
                } else {
                    None
                };
                limits.push(AccountBalanceLimit {
                    layer,
                    amount,
                    enforcement_direction,
                    start,
                    end,
                })
            }
            velocity_limits.push(AccountVelocityLimit {
                limit_id: velocity.id,
                window: velocity.window,
                condition: velocity.condition,
                currency: velocity.currency,
                limit: AccountLimit {
                    timestamp_source: velocity.limit.timestamp_source,
                    balance: limits,
                },
            });
        }
        Ok(velocity_limits)
    }
}
