mod account_control;
mod balance;
mod context;
mod control;
pub mod error;
mod limit;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use cala_types::{entry::EntryValues, transaction::TransactionValues};

pub use crate::param::Params;
use crate::{atomic_operation::*, outbox::*, primitives::AccountId};

use account_control::*;
use balance::*;
pub use control::*;
use error::*;
pub use limit::*;

#[derive(Clone)]
pub struct Velocities {
    outbox: Outbox,
    pool: PgPool,
    limits: VelocityLimitRepo,
    controls: VelocityControlRepo,
    account_controls: AccountControls,
    balances: VelocityBalances,
}

impl Velocities {
    pub(crate) fn new(pool: &PgPool, outbox: Outbox) -> Self {
        Self {
            limits: VelocityLimitRepo::new(pool),
            controls: VelocityControlRepo::new(pool),
            account_controls: AccountControls::new(pool),
            balances: VelocityBalances::new(pool),
            pool: pool.clone(),
            outbox,
        }
    }

    pub async fn create_limit(
        &self,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let limit = self.create_limit_in_op(&mut op, new_limit).await?;
        op.commit().await?;
        Ok(limit)
    }

    pub async fn create_limit_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        self.limits.create_in_tx(op.tx(), new_limit).await
    }

    pub async fn create_control(
        &self,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let control = self.create_control_in_op(&mut op, new_control).await?;
        op.commit().await?;
        Ok(control)
    }

    pub async fn create_control_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        self.controls.create_in_tx(op.tx(), new_control).await
    }

    pub async fn add_limit_to_control(
        &self,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let control = self
            .add_limit_to_control_in_op(&mut op, control, limit)
            .await?;
        op.commit().await?;
        Ok(control)
    }

    pub async fn add_limit_to_control_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<VelocityControl, VelocityError> {
        let db = op.tx();
        self.limits.add_limit_to_control(db, control, limit).await?;
        self.controls.find_by_id(db, control).await
    }

    pub async fn attach_control_to_account(
        &self,
        control: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let control = self
            .attach_control_to_account_in_op(&mut op, control, account_id, params)
            .await?;
        op.commit().await?;
        Ok(control)
    }

    pub async fn attach_control_to_account_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        control_id: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let control = self.controls.find_by_id(op.tx(), control_id).await?;
        let limits = self
            .limits
            .list_for_control(op.tx(), control_id)
            .await?
            .into_iter()
            .map(|l| l.into_values())
            .collect();

        self.account_controls
            .attach_control_in_op(
                op,
                control.created_at(),
                control.values(),
                account_id,
                limits,
                params,
            )
            .await?;
        Ok(control)
    }

    pub(crate) async fn update_balances_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        account_ids: &[AccountId],
    ) -> Result<(), VelocityError> {
        let controls = self
            .account_controls
            .find_for_enforcement(op, account_ids)
            .await?;

        self.balances
            .update_balances_in_op(op, created_at, transaction, entries, controls)
            .await
    }

    pub async fn list_limits_for_control(
        &self,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let limits = self
            .list_limits_for_control_in_op(&mut op, control_id)
            .await?;
        op.commit().await?;
        Ok(limits)
    }

    pub async fn list_limits_for_control_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        self.limits.list_for_control(op.tx(), control_id).await
    }

    pub async fn find_all_limits<T: From<VelocityLimit>>(
        &self,
        limit_ids: &[VelocityLimitId],
    ) -> Result<HashMap<VelocityLimitId, T>, VelocityError> {
        self.limits.find_all(limit_ids).await
    }

    pub async fn find_all_controls<T: From<VelocityControl>>(
        &self,
        control_ids: &[VelocityControlId],
    ) -> Result<HashMap<VelocityControlId, T>, VelocityError> {
        self.controls.find_all(control_ids).await
    }
}
