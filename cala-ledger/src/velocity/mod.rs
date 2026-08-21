mod account_control;
mod balance;
mod context;
mod control;
pub mod error;
mod limit;

use chrono::{DateTime, Utc};
use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use cala_types::{entry::EntryValues, transaction::TransactionValues};

pub use crate::param::Params;

pub(crate) use account_control::AccountVelocityControl;
use account_control::*;
use balance::*;
pub use control::*;
use error::*;
pub use limit::*;

#[derive(Clone)]
pub struct Velocities {
    limits: VelocityLimitRepo,
    controls: VelocityControlRepo,
    account_controls: AccountControls,
    balances: VelocityBalances,
    clock: ClockHandle,
}

impl Velocities {
    pub(crate) fn new(pool: &PgPool, clock: &ClockHandle) -> Self {
        Self {
            limits: VelocityLimitRepo::new(pool),
            controls: VelocityControlRepo::new(pool),
            account_controls: AccountControls::new(pool, clock),
            balances: VelocityBalances::new(pool, clock),
            clock: clock.clone(),
        }
    }

    #[instrument(name = "velocity.create_limit", skip_all)]
    pub async fn create_limit(
        &self,
        new_limit: NewVelocityLimit,
    ) -> Result<VelocityLimit, VelocityError> {
        let mut db = self.limits.begin_op_with_clock(&self.clock).await?;
        let limit = self.create_limit_in_op(&mut db, new_limit).await?;
        db.commit().await?;
        Ok(limit)
    }

    #[instrument(name = "velocity.create_limit_in_op", skip_all)]
    pub async fn create_limit_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
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
        let mut db = self.controls.begin_op_with_clock(&self.clock).await?;
        let control = self.create_control_in_op(&mut db, new_control).await?;
        db.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.create_control_in_op", skip_all)]
    pub async fn create_control_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
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
        let mut db = self.controls.begin_op_with_clock(&self.clock).await?;
        let control = self
            .add_limit_to_control_in_op(&mut db, control, limit)
            .await?;
        db.commit().await?;
        Ok(control)
    }

    #[instrument(name = "velocity.add_limit_to_control_in_op", skip(self, db), fields(control_id = %control, limit_id = %limit))]
    pub async fn add_limit_to_control_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control: VelocityControlId,
        limit: VelocityLimitId,
    ) -> Result<VelocityControl, VelocityError> {
        self.limits.add_limit_to_control(db, control, limit).await?;
        Ok(self.controls.find_by_id_in_op(db, control).await?)
    }

    #[instrument(level = "debug", name = "velocity.attach_control_to_account", skip(self), fields(control_id = %control, account_id = %account_id))]
    pub async fn attach_control_to_account(
        &self,
        control: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = self.controls.begin_op_with_clock(&self.clock).await?;
        let control = self
            .attach_control_to_account_or_account_set_in_op(&mut op, control, account_id, params)
            .await?;
        op.commit().await?;
        Ok(control)
    }

    #[instrument(level = "debug", name = "velocity.attach_control_to_account_set", skip(self), fields(control_id = %control, account_set_id = %account_set_id))]
    pub async fn attach_control_to_account_set(
        &self,
        control: VelocityControlId,
        account_set_id: AccountSetId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let mut op = self.controls.begin_op_with_clock(&self.clock).await?;
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

    #[instrument(level = "debug", name = "velocity.attach_control_to_account_in_op", skip(self, db), fields(control_id = %control_id, account_id = %account_id))]
    pub async fn attach_control_to_account_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control_id: VelocityControlId,
        account_id: AccountId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        self.attach_control_to_accounts_in_op(
            db,
            control_id,
            std::slice::from_ref(&account_id),
            params,
        )
        .await
    }

    /// Attach one control to N accounts in a fixed number of round trips
    /// (find control, list its limits, one batched insert) instead of `3 *
    /// account_ids.len()`.
    ///
    /// `params` is shared by every account in the batch — see the docs on
    /// [`AccountControls::attach_control_to_accounts_in_op`]. Intra-batch
    /// ordering is not observable: every account gets the same control,
    /// condition, and evaluated limits, so no caller can come to depend on
    /// an order here. `account_ids` is not deduplicated — attaching the
    /// same `account_id` twice in one call hits the same
    /// `UNIQUE(account_id, velocity_control_id)` constraint a second
    /// singular call would, and aborts the whole batch, not just that row.
    #[instrument(level = "debug", name = "velocity.attach_control_to_accounts_in_op", skip(self, db, account_ids), fields(control_id = %control_id, account_count = account_ids.len()), err(level = tracing::Level::WARN))]
    pub async fn attach_control_to_accounts_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control_id: VelocityControlId,
        account_ids: &[AccountId],
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        let control = self.controls.find_by_id_in_op(&mut *db, control_id).await?;
        let limits = self
            .limits
            .list_for_control(&mut *db, control_id)
            .await?
            .into_iter()
            .map(|l| l.into_values())
            .collect();

        self.account_controls
            .attach_control_to_accounts_in_op(db, control.values(), account_ids, limits, params)
            .await?;
        Ok(control)
    }

    #[instrument(level = "debug", name = "velocity.attach_control_to_account_set_in_op", skip(self, db), fields(control_id = %control_id, account_set_id = %account_set_id))]
    pub async fn attach_control_to_account_set_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        control_id: VelocityControlId,
        account_set_id: AccountSetId,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<VelocityControl, VelocityError> {
        self.attach_control_to_account_or_account_set_in_op(db, control_id, account_set_id, params)
            .await
    }

    #[instrument(level = "debug", name = "velocity.attach_control_internal", skip(self, db, account_id), fields(control_id = %control_id, account_id = tracing::field::Empty))]
    async fn attach_control_to_account_or_account_set_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
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
            .attach_control_in_op(db, control.values(), account_id, limits, params)
            .await?;
        Ok(control)
    }

    /// Enforce every matching velocity limit for one posting and write the
    /// resulting velocity balances.
    ///
    /// The controls are supplied by the caller: the posting flow reads the
    /// controls for its entry accounts as part of its single read statement
    /// (and those for any resolved ancestor sets in the ancestor phase), so the
    /// dedicated `find_for_enforcement` probe — which the pre-consolidation path
    /// issued on *every* posting, even in deployments with no velocity controls
    /// at all — is gone.
    ///
    /// Enforcement itself stays per posting: the evaluation context is built
    /// from one `TransactionValues`. Postings in a batch are enforced in input
    /// order within the same database transaction, so each one's read observes
    /// its predecessors' velocity writes — the same chaining a sequence of
    /// separate calls would produce.
    #[instrument(level = "debug", name = "velocity.enforce_batch_in_op", skip_all, fields(postings = postings.len()), err(level = tracing::Level::WARN))]
    pub(crate) async fn enforce_batch_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        created_at: DateTime<Utc>,
        postings: &[(&TransactionValues, &[EntryValues])],
        controls: &HashMap<AccountId, (VelocityContextAccountValues, Vec<AccountVelocityControl>)>,
        account_set_mappings: &crate::posting::AncestorMappings,
    ) -> Result<(), VelocityError> {
        self.balances
            .enforce_batch_in_op(db, created_at, postings, controls, account_set_mappings)
            .await
    }

    #[instrument(level = "debug", name = "velocity.list_limits_for_control", skip(self), fields(control_id = %control_id))]
    pub async fn list_limits_for_control(
        &self,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        let mut op = self.limits.begin_op_with_clock(&self.clock).await?;
        let limits = self
            .list_limits_for_control_in_op(&mut op, control_id)
            .await?;
        op.commit().await?;
        Ok(limits)
    }

    #[instrument(level = "debug", name = "velocity.list_limits_for_control_in_op", skip(self, op), fields(control_id = %control_id), err(level = tracing::Level::WARN))]
    pub async fn list_limits_for_control_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        control_id: VelocityControlId,
    ) -> Result<Vec<VelocityLimit>, VelocityError> {
        self.limits.list_for_control(op, control_id).await
    }

    #[instrument(level = "debug", name = "velocity.find_all_limits", skip(self, limit_ids), fields(count = limit_ids.len()), err(level = tracing::Level::WARN))]
    pub async fn find_all_limits<T: From<VelocityLimit>>(
        &self,
        limit_ids: &[VelocityLimitId],
    ) -> Result<HashMap<VelocityLimitId, T>, VelocityError> {
        Ok(self.limits.find_all(limit_ids).await?)
    }

    #[instrument(level = "debug", name = "velocity.find_all_controls", skip(self, control_ids), fields(count = control_ids.len()))]
    pub async fn find_all_controls<T: From<VelocityControl>>(
        &self,
        control_ids: &[VelocityControlId],
    ) -> Result<HashMap<VelocityControlId, T>, VelocityError> {
        Ok(self.controls.find_all(control_ids).await?)
    }
}

#[cfg(feature = "fuzz")]
mod __fuzz {
    //! Harness for the out-of-tree `velocity_enforce` fuzz target. Lives in
    //! this module so it can reach the `pub(super)` enforcement types.
    use super::account_control::AccountVelocityControl;
    use super::context::EvalContext;
    use cala_types::{
        balance::BalanceSnapshot, entry::EntryValues, transaction::TransactionValues,
        velocity::VelocityContextAccountValues,
    };
    use es_entity::clock::Clock;

    pub fn fuzz_enforce(data: &[u8]) {
        let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
        if parts.len() < 5 {
            return;
        }
        let Ok(control) = serde_json::from_slice::<AccountVelocityControl>(parts[0]) else {
            return;
        };
        let Ok(entry) = serde_json::from_slice::<EntryValues>(parts[1]) else {
            return;
        };
        let Ok(snapshot) = serde_json::from_slice::<BalanceSnapshot>(parts[2]) else {
            return;
        };
        let Ok(tx) = serde_json::from_slice::<TransactionValues>(parts[3]) else {
            return;
        };
        let Ok(account) = serde_json::from_slice::<VelocityContextAccountValues>(parts[4]) else {
            return;
        };

        // The account must be registered before we ask for its entry context
        // (context_for_entry expects it, panicking otherwise).
        let account_id = account.id;
        let mut eval = EvalContext::new(Clock::handle().clone(), &tx, std::iter::once(&account));
        let ctx = eval.context_for_entry(account_id, &entry);

        let _ = control.needs_enforcement(&ctx);
        for limit in &control.velocity_limits {
            let _ = limit.window_for_enforcement(&ctx, &entry);
            let _ = limit.enforce(&ctx, snapshot.created_at, &snapshot);
        }
    }
}

#[cfg(feature = "fuzz")]
pub use __fuzz::fuzz_enforce;
