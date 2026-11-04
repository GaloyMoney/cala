//! [Calculation]s maintain effective balance series per (calculation,
//! dimension tuple) for the accounts and sets they cover.
mod attachment;
pub mod effective;
mod entity;
pub mod error;
mod fold;
mod repo;

use es_entity::{
    clock::ClockHandle,
    errlanes::{lanes, Fail, ResultExt},
    Ensured,
};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::{
    error::CalaFault,
    outbox::{ObixOutbox, OutboxEventPayload, OutboxPublisher},
};

use attachment::*;
pub use attachment::{AttachmentByAccountIdCursor, CalculationAttachment};
pub use effective::{CalculationEffectiveBalances, CalculationSeriesCursor};
pub use entity::*;
use error::*;
pub(crate) use fold::CalculationRollupTxn;
use repo::*;

/// Service for working with `Calculation` entities and their series.
#[derive(Clone)]
pub struct Calculations {
    repo: CalculationRepo,
    attachments: AttachmentRepo,
    effective: CalculationEffectiveBalances,
    outbox: ObixOutbox,
    clock: ClockHandle,
    pool: PgPool,
}

impl Calculations {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher, clock: &ClockHandle) -> Self {
        Self {
            repo: CalculationRepo::new(pool, publisher),
            attachments: AttachmentRepo::new(pool),
            effective: CalculationEffectiveBalances::new(pool),
            outbox: publisher.inner().clone(),
            clock: clock.clone(),
            pool: pool.clone(),
        }
    }

    /// Creates the calculation. Its `activation_sequence` is the outbox
    /// frontier read before the insert: entries whose outbox sequence is at
    /// or below it are not in its series.
    #[es_entity::errlanes::instrument(name = "cala_ledger.calculation.create", skip_all)]
    pub async fn create(
        &self,
        new_calculation: NewCalculation,
    ) -> Result<Calculation, Fail<CreateCalculationRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let calculation = self.create_in_op(&mut op, new_calculation).await?;
        op.commit().await?;
        Ok(calculation)
    }

    /// [`Self::create`] within a caller-supplied operation.
    #[es_entity::errlanes::instrument(name = "cala_ledger.calculation.create_in_op", skip_all)]
    pub async fn create_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        new_calculation: NewCalculation,
    ) -> Result<Calculation, Fail<CreateCalculationRejection, lanes!(Transient, Fatal)>> {
        if !self
            .repo
            .journal_exists(op, new_calculation.journal_id)
            .await?
        {
            return Err(
                CreateCalculationRejection::JournalNotFound(new_calculation.journal_id).into(),
            );
        }
        let activation_sequence = self.activation_sequence().await?;
        let new_calculation = new_calculation.with_activation_sequence(activation_sequence);
        self.repo.create_in_op(op, new_calculation).await.lift()
    }

    /// Creates the calculation, or returns the stored calculation its id or
    /// `code` matches, unchanged.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.ensure_in_op",
        skip_all
    )]
    pub async fn ensure_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        new_calculation: NewCalculation,
    ) -> Result<Ensured<Calculation>, CalaFault> {
        let activation_sequence = self.activation_sequence().await?;
        let new_calculation = new_calculation.with_activation_sequence(activation_sequence);
        self.repo
            .create_or_find_in_op(op, new_calculation)
            .await
            .narrow_rejected()
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.find_by_id",
        skip(self)
    )]
    pub async fn find_by_id(
        &self,
        id: CalculationId,
    ) -> Result<Calculation, Fail<CalculationNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_id(id)
            .await?
            .ok_or(CalculationNotFound(id))?)
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.find_by_code",
        skip(self)
    )]
    pub async fn find_by_code(
        &self,
        code: &str,
    ) -> Result<Calculation, Fail<CalculationCodeNotFound, lanes!(Transient, Fatal)>> {
        Ok(self
            .repo
            .maybe_find_by_code(code.to_owned())
            .await?
            .ok_or_else(|| CalculationCodeNotFound(code.to_owned()))?)
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.find_all", skip(self, ids), fields(ids_count = ids.len()))]
    pub async fn find_all<T: From<Calculation>>(
        &self,
        ids: &[CalculationId],
    ) -> Result<HashMap<CalculationId, T>, CalaFault> {
        self.repo.find_all(ids).await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.list_for_journal",
        skip(self)
    )]
    pub async fn list_for_journal(
        &self,
        journal_id: JournalId,
    ) -> Result<Vec<Calculation>, CalaFault> {
        self.repo.list_for_journal(&self.pool, journal_id).await
    }

    /// Attach the calculation to an account or account set. Rejected for a
    /// `Global` calculation, for a set in another journal and for a target
    /// already attached. The attachment's `activation_sequence` is the
    /// outbox frontier read before the insert: entries whose outbox
    /// sequence is at or below it are not in the target's series.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.attach",
        skip(self, target)
    )]
    pub async fn attach(
        &self,
        id: CalculationId,
        target: impl Into<AccountId>,
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        self.attach_in_op(&mut op, id, target).await?;
        op.commit().await?;
        Ok(())
    }

    /// [`Self::attach`] within a caller-supplied operation.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.attach_in_op",
        skip(self, op, target)
    )]
    pub async fn attach_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        target: impl Into<AccountId>,
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        self.attach_many_in_op(op, id, &[target.into()]).await
    }

    /// [`Self::attach`] for many targets, with one batched `INSERT`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.attach_many_in_op", skip(self, op, targets), fields(targets_count = targets.len()))]
    pub async fn attach_many_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        targets: &[AccountId],
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        let activation_sequence = self.validate_attach_in_op(op, id, targets).await?;
        self.attachments
            .create_all_in_op(op, id, targets, activation_sequence)
            .await
            .lift()?;
        Ok(())
    }

    /// Idempotent [`Self::attach_in_op`]: returns `Found` when the target
    /// is already attached.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.ensure_attached_in_op",
        skip(self, op, target)
    )]
    pub async fn ensure_attached_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        target: impl Into<AccountId>,
    ) -> Result<Ensured<()>, CalaFault> {
        let target = target.into();
        let activation_sequence = self
            .validate_attach_in_op(op, id, &[target])
            .await
            .narrow_rejected()?;
        self.attachments
            .ensure_in_op(op, id, target, activation_sequence)
            .await
    }

    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.list_attachments",
        skip(self)
    )]
    pub async fn list_attachments(
        &self,
        id: CalculationId,
        args: es_entity::PaginatedQueryArgs<AttachmentByAccountIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<CalculationAttachment, AttachmentByAccountIdCursor>,
        CalaFault,
    > {
        self.attachments.list_for_calculation(id, args).await
    }

    pub fn effective(&self) -> &CalculationEffectiveBalances {
        &self.effective
    }

    /// Checks the attach rules and returns the activation sequence for the
    /// new attachment rows.
    async fn validate_attach_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        targets: &[AccountId],
    ) -> Result<u64, Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        let calculation = self
            .repo
            .maybe_find_by_id_in_op(&mut *op, id)
            .await?
            .ok_or(AttachCalculationRejection::CalculationNotFound(id))?;
        let values = calculation.values();
        if values.scope == CalculationScope::Global {
            return Err(AttachCalculationRejection::GlobalCannotAttach(id).into());
        }
        for target in self.attachments.resolve_targets(op, targets).await? {
            if !target.exists {
                return Err(AttachCalculationRejection::TargetNotFound(target.account_id).into());
            }
            if let Some(target_journal) = target.set_journal_id {
                if target_journal != values.journal_id {
                    return Err(AttachCalculationRejection::JournalMismatch {
                        target: target.account_id,
                        target_journal,
                        calculation_journal: values.journal_id,
                    }
                    .into());
                }
            }
        }
        Ok(self.activation_sequence().await?)
    }

    async fn activation_sequence(&self) -> Result<u64, CalaFault> {
        let frontier = self
            .outbox
            .frontier::<obix::out::InsertOrder>()
            .await
            .narrow_rejected()?;
        Ok(u64::from(frontier))
    }
}

impl From<&CalculationEvent> for OutboxEventPayload {
    fn from(event: &CalculationEvent) -> Self {
        match event {
            CalculationEvent::Initialized {
                values: calculation,
            } => OutboxEventPayload::CalculationCreated {
                calculation: calculation.clone(),
            },
        }
    }
}
