//! [Calculation]s maintain effective balance series per (calculation,
//! dimension tuple) for the accounts and sets they cover.
mod attachment;
mod backfill;
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
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::{
    balance::Balances,
    error::CalaFault,
    outbox::{ObixOutbox, OutboxPublisher},
};

use attachment::*;
pub use attachment::{AttachmentByAccountIdCursor, CalculationAttachment};
use backfill::BackfillRepo;
pub use backfill::CalculationBackfill;
pub use effective::{CalculationEffectiveBalances, CalculationSeriesCursor};
pub use entity::*;
use error::*;
pub(crate) use fold::{CalculationRollupTxn, RollupScope};
use repo::*;

/// Advisory lock class that orders the tip's flush against a backfill handoff.
/// Existing classes are 1..=5 (balance, member, graph, id, external id).
pub(crate) const CALCULATION_HANDOFF_LOCK_CLASS: i32 = 6;

/// Service for working with `Calculation` entities and their series.
#[derive(Clone)]
pub struct Calculations {
    repo: CalculationRepo,
    attachments: AttachmentRepo,
    backfills: BackfillRepo,
    effective: CalculationEffectiveBalances,
    balances: Balances,
    outbox: ObixOutbox,
    clock: ClockHandle,
    pool: PgPool,
}

/// The targets of an attach call, validated.
struct ValidatedAttach {
    activation_sequence: u64,
    journal_id: JournalId,
    accounts: Vec<AccountId>,
    sets: HashSet<AccountId>,
}

impl Calculations {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        balances: &Balances,
        clock: &ClockHandle,
    ) -> Self {
        Self {
            repo: CalculationRepo::new(pool, publisher),
            attachments: AttachmentRepo::new(pool),
            backfills: BackfillRepo::new(pool),
            effective: CalculationEffectiveBalances::new(pool),
            balances: balances.clone(),
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
        let starts_backfill = new_calculation.starts_backfill();
        let new_calculation = new_calculation.with_activation_sequence(activation_sequence);
        let calculation: Calculation = self.repo.create_in_op(op, new_calculation).await.lift()?;
        if starts_backfill {
            self.start_backfill_in_op(op, calculation.id(), None)
                .await?;
        }
        Ok(calculation)
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
        let starts_backfill = new_calculation.starts_backfill();
        let new_calculation = new_calculation.with_activation_sequence(activation_sequence);
        let ensured = self
            .repo
            .create_or_find_in_op(op, new_calculation)
            .await
            .narrow_rejected()?;
        if let (Ensured::Created(calculation), true) = (&ensured, starts_backfill) {
            self.start_backfill_in_op(op, calculation.id(), None)
                .await?;
        }
        Ok(ensured)
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
        self.attach_with_history_in_op(op, id, target, History::default())
            .await
    }

    /// [`Self::attach_in_op`] with an explicit [`History`].
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.attach_with_history_in_op",
        skip(self, op, target)
    )]
    pub async fn attach_with_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        target: impl Into<AccountId>,
        history: History,
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        self.attach_many_with_history_in_op(op, id, &[target.into()], history)
            .await
    }

    /// [`Self::attach`] for many targets, with one batched `INSERT`.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.attach_many_in_op", skip(self, op, targets), fields(targets_count = targets.len()))]
    pub async fn attach_many_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        targets: &[AccountId],
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        self.attach_many_with_history_in_op(op, id, targets, History::default())
            .await
    }

    /// [`Self::attach_many_in_op`] with an explicit [`History`]. With
    /// [`History::Backfill`], a plain account without entries is `Complete`
    /// at once; every other target is `InProgress` and the targets of one
    /// call share one backfill.
    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.calculation.attach_many_with_history_in_op", skip(self, op, targets), fields(targets_count = targets.len()))]
    pub async fn attach_many_with_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        targets: &[AccountId],
        history: History,
    ) -> Result<(), Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        let validated = self.validate_attach_in_op(op, id, targets).await?;
        let with_history: HashSet<AccountId> =
            if history == History::Backfill && !validated.accounts.is_empty() {
                let pairs: Vec<(JournalId, AccountId)> = validated
                    .accounts
                    .iter()
                    .map(|account_id| (validated.journal_id, *account_id))
                    .collect();
                self.balances
                    .members_with_balance_history_in_op(op, &pairs)
                    .await?
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            };
        let rows: Vec<(AccountId, bool)> = targets
            .iter()
            .map(|target| {
                let needs_backfill = history == History::Backfill
                    && (validated.sets.contains(target) || with_history.contains(target));
                (*target, !needs_backfill)
            })
            .collect();
        self.attachments
            .create_all_in_op(op, id, &rows, validated.activation_sequence)
            .await
            .lift()?;
        let backfill_targets: Vec<AccountId> = rows
            .iter()
            .filter(|(_, complete)| !complete)
            .map(|(target, _)| *target)
            .collect();
        if !backfill_targets.is_empty() {
            self.start_backfill_in_op(op, id, Some(&backfill_targets))
                .await?;
        }
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
        self.ensure_attached_with_history_in_op(op, id, target, History::default())
            .await
    }

    /// [`Self::ensure_attached_in_op`] with an explicit [`History`].
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.ensure_attached_with_history_in_op",
        skip(self, op, target)
    )]
    pub async fn ensure_attached_with_history_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        target: impl Into<AccountId>,
        history: History,
    ) -> Result<Ensured<()>, CalaFault> {
        let target = target.into();
        let validated = self
            .validate_attach_in_op(op, id, &[target])
            .await
            .narrow_rejected()?;
        let needs_backfill = history == History::Backfill
            && (validated.sets.contains(&target)
                || self
                    .balances
                    .member_has_balance_history_in_op(op, validated.journal_id, target)
                    .await?);
        let ensured = self
            .attachments
            .ensure_in_op(
                op,
                id,
                target,
                validated.activation_sequence,
                !needs_backfill,
            )
            .await?;
        if needs_backfill && matches!(ensured, Ensured::Created(())) {
            self.start_backfill_in_op(op, id, Some(&[target])).await?;
        }
        Ok(ensured)
    }

    /// Progress of the open backfills of one calculation. Lag is
    /// `calculation_rollup_status().applied - cursor`, computed by the caller.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.list_open_backfills",
        skip(self)
    )]
    pub async fn list_open_backfills(
        &self,
        id: CalculationId,
    ) -> Result<Vec<CalculationBackfill>, CalaFault> {
        self.backfills.list_open(id).await
    }

    /// Block until the unit is `Complete`. `target = None` fences a Global
    /// calculation, `Some(account)` fences that attachment. A target that is
    /// not attached is an unmet target and times out.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.calculation.await_backfill",
        skip(self)
    )]
    pub async fn await_backfill(
        &self,
        id: CalculationId,
        target: Option<AccountId>,
        timeout: Duration,
    ) -> Result<(), Fail<CalculationBackfillTimeout, lanes!(Transient, Fatal)>> {
        let started = tokio::time::Instant::now();
        let mut delay = Duration::from_millis(100);
        loop {
            let complete = match target {
                None => self
                    .repo
                    .maybe_find_by_id(id)
                    .await?
                    .is_some_and(|c| c.values().backfill_status == BackfillStatus::Complete),
                Some(account_id) => self
                    .attachments
                    .backfill_complete(id, account_id)
                    .await?
                    .unwrap_or(false),
            };
            if complete {
                return Ok(());
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Err(CalculationBackfillTimeout {
                    calculation_id: id,
                    target,
                    elapsed,
                }
                .into());
            }
            tokio::time::sleep(delay.min(timeout - elapsed)).await;
            delay = (delay * 2).min(Duration::from_millis(250));
        }
    }

    async fn start_backfill_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        calculation_id: CalculationId,
        account_ids: Option<&[AccountId]>,
    ) -> Result<(), CalaFault> {
        self.backfills
            .create_in_op(
                op,
                CalculationBackfillId::new(),
                calculation_id,
                account_ids,
            )
            .await
    }

    pub(crate) async fn lock_handoff_shared_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
    ) -> Result<(), CalaFault> {
        self.backfills.lock_handoff_shared_in_op(op).await
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

    /// Checks the attach rules and classifies the targets; carries the
    /// activation sequence for the new attachment rows.
    async fn validate_attach_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: CalculationId,
        targets: &[AccountId],
    ) -> Result<ValidatedAttach, Fail<AttachCalculationRejection, lanes!(Transient, Fatal)>> {
        let calculation = self
            .repo
            .maybe_find_by_id_in_op(&mut *op, id)
            .await?
            .ok_or(AttachCalculationRejection::CalculationNotFound(id))?;
        let values = calculation.values();
        if values.scope == CalculationScope::Global {
            return Err(AttachCalculationRejection::GlobalCannotAttach(id).into());
        }
        let journal_id = values.journal_id;
        let mut accounts = Vec::new();
        let mut sets = HashSet::new();
        for target in self.attachments.resolve_targets(op, targets).await? {
            if !target.exists {
                return Err(AttachCalculationRejection::TargetNotFound(target.account_id).into());
            }
            match target.set_journal_id {
                Some(target_journal) if target_journal != journal_id => {
                    return Err(AttachCalculationRejection::JournalMismatch {
                        target: target.account_id,
                        target_journal,
                        calculation_journal: journal_id,
                    }
                    .into());
                }
                Some(_) => {
                    sets.insert(target.account_id);
                }
                None => accounts.push(target.account_id),
            }
        }
        Ok(ValidatedAttach {
            activation_sequence: self.activation_sequence().await?,
            journal_id,
            accounts,
            sets,
        })
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
