mod account_control;
mod balance;
mod context;
mod control;
pub mod error;
mod limit;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use cala_types::{entry::EntryValues, transaction::TransactionValues};

pub use crate::param::Params;
use crate::{ledger_operation::*, outbox::*};

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

    #[instrument(name = "velocity.create_limit", skip_all)]
    pub async fn create_limit(
        &self,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let mut db = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let limit = self.create_limit_in_op(&mut db, new_limit).await?;
        db.commit().await?;
        Ok(limit)
    }

    #[instrument(name = "velocity.create_limit_in_op", skip_all)]
    pub async fn create_limit_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let res = self.limits.create_in_op(db, new_limit).await?;
        Ok(res)
    }

    #[instrument(name = "velocity.create_control", skip_all)]
    pub async fn create_control(
        &self,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        let mut db = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let control = self.create_control_in_op(&mut db, new_control).await?;
        db.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.create_control_in_op", skip_all)]
    pub async fn create_control_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        new_control: NewVelocityControl,
    ) -> Result<VelocityControl, VelocityError> {
        let res = self.controls.create_in_op(db, new_control).await?;
        Ok(res)
    }

    #[instrument(name = "velocity.add_limit_to_control", skip(self), fields(control_id = %control, limit_id = %limit))]
    pub async fn add_limit_to_control(
        &self,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<VelocityControl, VelocityError> {
        let mut db = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let control = self
            .add_limit_to_control_in_op(&mut db, control, limit)
            .await?;
        db.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.add_limit_to_control_in_op", skip(self, db), fields(control_id = %control, limit_id = %limit))]
    pub async fn add_limit_to_control_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<VelocityControl, VelocityError> {
        self.limits.add_limit_to_control(db, control, limit).await?;
        self.controls.find_by_id_in_op(db, control).await
    }

    #[instrument(name = "velocity.attach_control_to_account", skip(self), fields(control_id = %control, account_id = %account_id))]
    pub async fn attach_control_to_account(
        &self,
        control: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let control = self
            .attach_control_to_account_or_account_set_in_op(&mut op, control, account_id, params)
            .await?;
        op.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.attach_control_to_account_set", skip(self), fields(control_id = %control, account_set_id = %account_set_id))]
    pub async fn attach_control_to_account_set(
        &self,
        control: VelocityControlId,
        account_set_id: AccountSetId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let control = self
            .attach_control_to_account_or_account_set_in_op(
                &mut op,
                control,
                account_set_id,
                params,
            )
            .await?;
        op.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.attach_control_to_account_in_op", skip(self, db), fields(control_id = %control_id, account_id = %account_id))]
    pub async fn attach_control_to_account_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        control_id: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        self.attach_control_to_account_or_account_set_in_op(db, control_id, account_id, params)
            .await
    }

    #[instrument(name = "velocity.attach_control_to_account_set_in_op", skip(self, db), fields(control_id = %control_id, account_set_id = %account_set_id))]
    pub async fn attach_control_to_account_set_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        control_id: VelocityControlId,
        account_set_id: AccountSetId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        self.attach_control_to_account_or_account_set_in_op(db, control_id, account_set_id, params)
            .await
    }

    #[instrument(name = "velocity.attach_control_internal", skip(self, db, account_id), fields(control_id = %control_id, account_id = tracing::field::Empty))]
    async fn attach_control_to_account_or_account_set_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        control_id: VelocityControlId,
        account_id: impl Into<AccountId>,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let account_id = account_id.into();
        tracing::Span::current().record("account_id", account_id.to_string());

        let control = self.controls.find_by_id_in_op(&mut *db, control_id).await?;
        let limits = self
            .limits
            .list_for_control(&mut *db, control_id)
            .await?
            .into_iter()
            .map(|l| l.into_values())
            .collect();

        self.account_controls
            .attach_control_in_op(
                db,
                control.created_at(),
                control.values(),
                account_id,
                limits,
                params,
            )
            .await?;
        Ok(control)
    }

    #[instrument(name = "velocity.update_balances_with_limit_enforcement_in_op", skip(self, db, transaction, entries, account_set_mappings), fields(account_ids_count = account_ids.len(), entries_count = entries.len()), err)]
    pub(crate) async fn update_balances_with_limit_enforcement_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        created_at: DateTime<Utc>,
        transaction: &TransactionValues,
        entries: &[EntryValues],
        account_ids: &[AccountId],
        account_set_mappings: &HashMap<AccountId, Vec<AccountSetId>>,
    ) -> Result<(), VelocityError> {
        let mut all_account_ids = account_ids.to_vec();
        all_account_ids.extend(
            account_ids
                .iter()
                .filter_map(|id| account_set_mappings.get(id))
                .flat_map(|ids| ids.iter().map(AccountId::from)),
        );

        let controls = self
            .account_controls
            .find_for_enforcement(db, &all_account_ids)
            .await?;

        self.balances
            .update_balances_with_limit_enforcement_in_op(
                db,
                created_at,
                transaction,
                entries,
                controls,
                account_set_mappings,
            )
            .await
    }

    #[instrument(name = "velocity.list_limits_for_control", skip(self), fields(control_id = %control_id))]
    pub async fn list_limits_for_control(
        &self,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let limits = self
            .list_limits_for_control_in_op(&mut op, control_id)
            .await?;
        op.commit().await?;
        Ok(limits)
    }

    #[instrument(name = "velocity.list_limits_for_control_in_op", skip(self, op), fields(control_id = %control_id), err)]
    pub async fn list_limits_for_control_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        self.limits.list_for_control(op, control_id).await
    }

    #[instrument(name = "velocity.find_all_limits", skip(self), fields(count = limit_ids.len()), err)]
    pub async fn find_all_limits<T: From<VelocityLimit>>(
        &self,
        limit_ids: &[VelocityLimitId],
    ) -> Result<HashMap<VelocityLimitId, T>, VelocityError> {
        self.limits.find_all(limit_ids).await
    }

    #[instrument(name = "velocity.find_all_controls", skip(self), fields(count = control_ids.len()))]
    pub async fn find_all_controls<T: From<VelocityControl>>(
        &self,
        control_ids: &[VelocityControlId],
    ) -> Result<HashMap<VelocityControlId, T>, VelocityError> {
        self.controls.find_all(control_ids).await
    }
}
