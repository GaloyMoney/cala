//! The posting flow: transactions in, rows out, in a fixed number of phases.
//!
//! # Shape
//!
//! Every statement takes arrays, so posting one transaction is the N=1 case of
//! posting a batch. The simple path — no account-set membership, no velocity
//! controls, no effective balances — costs **six round trips regardless of
//! batch size**:
//!
//! | # | phase | statement |
//! |---|-------|-----------|
//! | 1 | | `BEGIN` |
//! | 2 | lock (the fence) | [`PostingRepo::lock_balances_and_probe_templates_in_op`] |
//! | 3 | read | [`PostingRepo::read_posting_state_in_op`] |
//! | 4 | write | [`PostingRepo::insert_postings_and_balances_in_op`] |
//! | 5 | | outbox insert (obix commit hook) |
//! | 6 | | `COMMIT` |
//!
//! Phase 2 takes the union advisory locks, pins `now()`, and probes the
//! template versions preparation used. Phase 3 reads memberships, the set-graph
//! epoch, journals, account metadata, velocity controls and balances. Phase 4
//! writes transactions, entries, both event streams and the balance snapshots.
//!
//! Between 3 and 4 the flow runs entirely in memory: ancestor expansion against
//! the set-graph cache, the chained balance fold, and velocity enforcement.
//! Nothing is written until all of it has succeeded, which is what lets a
//! rejection name the posting that caused it with no rows to undo.
//!
//! Features a deployment actually uses cost extra statements, and only then —
//! but they too are **per batch, not per posting**:
//!
//! - non-EC ancestor sets: a lock statement and a supplemental read, one pair
//!   per journal involved;
//! - velocity: three statements (lock, read, write) for the whole batch, and
//!   *zero* when no limit's window matches — the check that decides is pure CEL
//!   over controls the read statement already returned;
//! - effective balances: two statements per distinct `(journal, effective
//!   date)`, which for the usual same-day batch is two for the batch.
//!
//! So a batch of 10 with velocity and effective balances both live costs ~1.15
//! statements per posting, against 11 when posted one at a time.
//!
//! # What bounds a batch
//!
//! Not its size — 500k postings over a small account pool is fine. The fence
//! holds two advisory locks per distinct entry account until commit, and those
//! live in Postgres' *shared* lock table, so the limit is the number of
//! distinct `(journal, account, currency)` balances a batch touches. Past
//! [`error::MAX_DISTINCT_BALANCES_PER_BATCH`] the flow refuses up front rather
//! than letting Postgres raise a bare `out of shared memory` — which names
//! neither cause nor fix, and can strike unrelated concurrent transactions.
//!
//! # Ordering within a batch
//!
//! The result is exactly as if the postings had run one at a time, in input
//! order, inside one transaction: later postings observe earlier postings'
//! balances, velocity limits enforce against the chained snapshots, and
//! snapshot versions increment in order. All postings share one `created_at`
//! (the transaction timestamp); entry `sequence` and snapshot `version` carry
//! intra-batch order, exactly as two postings landing in the same millisecond
//! do today.
//!
//! Balance folding is grouped by journal — balances are keyed per journal, so
//! per-journal folding is equivalent to a global one and lets a batch span
//! journals without the fold needing a journal-aware key.
//!
//! # Layering
//!
//! Strictly service -> cache -> repo, mirroring `account_set`: [`Postings`]
//! orchestrates and holds the domain services whose in-memory logic it reuses;
//! [`TemplateCache`] decides cache-or-DB for template bodies;
//! [`PostingRepo`] owns every statement the flow issues. Template CEL
//! evaluation itself is the template domain's logic and stays in
//! [`TxTemplates::prepare_transaction`].

use crate::error::CalaFault;
use es_entity::errlanes::{lanes, Fail, ResultExt};
pub(crate) mod error;
mod repo;
mod template_cache;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use es_entity::AtomicOperation;

use cala_types::{balance::BalanceSnapshot, entry::EntryValues};

use crate::{
    account_set::AccountSets,
    balance::{error::BalanceAccountLocked, Balances},
    outbox::OutboxPublisher,
    primitives::*,
    transaction::Transaction,
    tx_template::{Params, PreparedTransaction, TxTemplates},
    velocity::Velocities,
};

pub use error::{
    ApplyPostingRejection, BatchPostingRejection, BatchPreparePostingRejection, PostingRef,
    PostingRejection, PreparePostingRejection, TooManyPostingBalances, ValidatePostingRejection,
};

/// Ancestor account sets per journal: `journal -> (leaf -> its sets in that
/// journal)`. Keyed by journal because a leaf account has no journal of its
/// own and may belong to sets in several — see
/// [`Postings::resolve_ancestors`].
pub(crate) type AncestorMappings = HashMap<JournalId, HashMap<AccountId, Vec<AccountSetId>>>;

use repo::{BalanceKeys, PostingRepo, PostingRows, PostingState};
use template_cache::{ResolvedTemplate, TemplateCache};

/// One transaction to post.
#[derive(Debug, Clone)]
pub struct PostingInput {
    pub tx_id: TransactionId,
    pub tx_template_code: String,
    pub params: Params,
}

impl PostingInput {
    pub fn new(
        tx_id: TransactionId,
        tx_template_code: impl Into<String>,
        params: impl Into<Params>,
    ) -> Self {
        Self {
            tx_id,
            tx_template_code: tx_template_code.into(),
            params: params.into(),
        }
    }
}

/// The posting flow, owning the hot-path SQL that spans domain boundaries.
///
/// Holds handles on the domain services whose in-memory logic it reuses —
/// template evaluation, ancestor resolution, velocity enforcement,
/// effective-balance maintenance — but issues the transaction/entry/balance
/// statements itself (via [`PostingRepo`]), because fusing them is the point.
#[derive(Clone)]
pub struct Postings {
    repo: PostingRepo,
    tx_templates: TxTemplates,
    account_sets: AccountSets,
    balances: Balances,
    velocities: Velocities,
    publisher: OutboxPublisher,
    templates: TemplateCache,
}

impl Postings {
    pub(crate) fn new(
        publisher: &OutboxPublisher,
        tx_templates: &TxTemplates,
        account_sets: &AccountSets,
        balances: &Balances,
        velocities: &Velocities,
    ) -> Self {
        let repo = PostingRepo;
        Self {
            templates: TemplateCache::new(repo.clone()),
            repo,
            tx_templates: tx_templates.clone(),
            account_sets: account_sets.clone(),
            balances: balances.clone(),
            velocities: velocities.clone(),
            publisher: publisher.clone(),
        }
    }

    /// Post a batch of transactions in one operation, all-or-nothing.
    ///
    /// Any failure aborts the whole batch; the error names the offending
    /// posting. See the module docs for the ordering guarantees within a batch.
    // `debug`, not the default INFO: this sits under every
    // `post_transaction` / `post_transactions` unit-of-work span, so at INFO
    // each posting would pay for a second exported span on the hot path.
    #[es_entity::errlanes::instrument(
        level = "debug",
        name = "cala_ledger.posting.post_all_in_op",
        skip_all,
        fields(
            batch_size = batch.len(),
            failed_posting_index = tracing::field::Empty,
            failed_posting_id = tracing::field::Empty,
        )
    )]
    pub(crate) async fn post_all_in_op(
        &self,
        db: &mut impl AtomicOperation,
        batch: Vec<PostingInput>,
    ) -> Result<Vec<Transaction>, Fail<BatchPostingRejection, lanes!(Transient, Fatal)>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let codes = Self::dedup(batch.iter().map(|p| p.tx_template_code.clone()));
        let mut used = self.templates.resolve_in_op(db, &codes).await.lift()?;
        let mut prepared = self.prepare_all(&batch, &used)?;
        let (mut keys, locked) = self.lock_prepared(db, &prepared, &codes).await.lift()?;
        // A stale template must be refreshed, then prepared again before reading.
        // Supplemental locks retain the original order and SQL footprint.
        if let Err(stale) = TemplateCache::assert_up_to_date(&used, &locked.template_versions) {
            used.extend(self.templates.refresh_in_op(db, &stale).await.lift()?);
            prepared = self.prepare_all(&batch, &used)?;
            keys = Self::entry_balance_keys(&prepared);
            self.repo
                .lock_balances_and_probe_templates_in_op(db, &keys, &[], db.maybe_now())
                .await?;
        }
        let read = self.read_prepared(db, &prepared, &keys).await?;
        self.validate_batch(&batch, &prepared, &read)?;
        let transactions = self
            .apply_prepared(db, prepared, read, locked.now)
            .await
            .lift()?;
        Ok(transactions)
    }

    #[es_entity::errlanes::instrument(level = "debug", name = "cala_ledger.posting.post_in_op", skip_all,
        fields(failed_posting_index = tracing::field::Empty, failed_posting_id = tracing::field::Empty))]
    pub(crate) async fn post_in_op(
        &self,
        db: &mut impl AtomicOperation,
        input: PostingInput,
    ) -> Result<Transaction, Fail<PostingRejection, lanes!(Transient, Fatal)>> {
        let codes = vec![input.tx_template_code.clone()];
        let mut used = self.templates.resolve_in_op(db, &codes).await.lift()?;
        let mut prepared = vec![self.prepare_one(
            &input,
            &used,
            PostingRef {
                index: 0,
                tx_id: input.tx_id,
            },
        )?];
        let (mut keys, locked) = self.lock_prepared(db, &prepared, &codes).await.lift()?;
        // A stale template must be refreshed, then prepared again before reading.
        // Supplemental locks retain the original order and SQL footprint.
        if let Err(stale) = TemplateCache::assert_up_to_date(&used, &locked.template_versions) {
            used.extend(self.templates.refresh_in_op(db, &stale).await.lift()?);
            prepared = vec![self.prepare_one(
                &input,
                &used,
                PostingRef {
                    index: 0,
                    tx_id: input.tx_id,
                },
            )?];
            keys = Self::entry_balance_keys(&prepared);
            self.repo
                .lock_balances_and_probe_templates_in_op(db, &keys, &[], db.maybe_now())
                .await?;
        }
        let read = self.read_prepared(db, &prepared, &keys).await?;
        Self::validate_one(
            &prepared[0],
            &read,
            PostingRef {
                index: 0,
                tx_id: input.tx_id,
            },
        )?;
        let transactions = self
            .apply_prepared(db, prepared, read, locked.now)
            .await
            .lift()?;
        Ok(transactions
            .into_iter()
            .next()
            .expect("one posting in, one transaction out"))
    }

    // Supplemental locks after re-preparation use the same key class as the
    // locks already held. A concurrent template update can therefore create
    // the same rare deadlock window as before; PostgreSQL aborts one side in
    // the native retryable lane. Neither path changes the canonical SQL lock
    // ordering or adds a per-item database loop.
    async fn lock_prepared(
        &self,
        db: &mut impl AtomicOperation,
        prepared: &[PreparedTransaction],
        codes: &[String],
    ) -> Result<
        (BalanceKeys, repo::LockOutcome),
        Fail<PreparePostingRejection, lanes!(Transient, Fatal)>,
    > {
        let keys = Self::entry_balance_keys(prepared);
        Self::check_balance_budget(&keys).lift()?;
        let locked = self
            .repo
            .lock_balances_and_probe_templates_in_op(db, &keys, codes, db.maybe_now())
            .await?;
        Ok((keys, locked))
    }

    fn check_balance_budget(keys: &BalanceKeys) -> Result<(), TooManyPostingBalances> {
        if keys.account_ids.len() > error::MAX_DISTINCT_BALANCES_PER_BATCH {
            return Err(TooManyPostingBalances {
                distinct: keys.account_ids.len(),
                max: error::MAX_DISTINCT_BALANCES_PER_BATCH,
            });
        }
        Ok(())
    }

    async fn read_prepared(
        &self,
        db: &mut impl AtomicOperation,
        prepared: &[PreparedTransaction],
        keys: &BalanceKeys,
    ) -> Result<PostingState, CalaFault> {
        let account_ids = Self::dedup(
            prepared
                .iter()
                .flat_map(|p| p.entries.iter().map(|e| e.account_id())),
        );
        let journal_ids = Self::dedup(prepared.iter().map(|p| p.journal_id));
        self.repo
            .read_posting_state_in_op(db, &account_ids, &journal_ids, keys)
            .await
    }

    async fn apply_prepared(
        &self,
        db: &mut impl AtomicOperation,
        prepared: Vec<PreparedTransaction>,
        mut read: PostingState,
        now: DateTime<Utc>,
    ) -> Result<Vec<Transaction>, Fail<ApplyPostingRejection, lanes!(Transient, Fatal)>> {
        // ---- ancestor phase (only when memberships exist) --------------
        let mappings = self
            .resolve_ancestors(db, &prepared, &mut read)
            .await
            .lift()?;

        // ---- fold + enforce (client-side) ------------------------------
        let (transactions, entries_per_posting) = prepared
            .into_iter()
            .map(|p| p.into_new_transaction(now))
            .collect::<(Vec<_>, Vec<_>)>();

        let mut hydrated = Vec::with_capacity(transactions.len());
        let mut entry_values: Vec<Vec<EntryValues>> = Vec::with_capacity(transactions.len());
        let mut rows = PostingRows::default();

        for (new_tx, new_entries) in transactions.into_iter().zip(entries_per_posting) {
            let transaction = rows.push_transaction(new_tx, now);
            let mut values = Vec::with_capacity(new_entries.len());
            for new_entry in new_entries {
                let entry = rows.push_entry(new_entry, now);
                values.push(entry.into_values());
            }
            entry_values.push(values);
            hydrated.push(transaction);
        }

        let snapshots = self.fold_balances(&hydrated, &entry_values, &read, &mappings, now);

        // Velocity for the whole batch: one lock, one read, one write — or
        // nothing at all, when no limit's window matches (the common case for
        // a deployment with controls attached to only some accounts).
        let for_enforcement: Vec<(&cala_types::transaction::TransactionValues, &[EntryValues])> =
            hydrated
                .iter()
                .zip(entry_values.iter())
                .map(|(tx, values)| (tx.values(), values.as_slice()))
                .collect();
        self.velocities
            .enforce_batch_in_op(db, now, &for_enforcement, &read.controls, &mappings)
            .await
            .lift()?;

        // ---- phase 3: apply --------------------------------------------
        self.repo
            .insert_postings_and_balances_in_op(db, now, &rows, &snapshots)
            .await
            .classify::<error::PostWrite>()?;

        self.update_effective_balances(db, &hydrated, &entry_values, &read, &mappings, now)
            .await?;

        // ---- publish ----------------------------------------------------
        // Per posting: the transaction event, then its entry events — the
        // interleaving a sequence of single-posting calls produces.
        let mut payloads = Vec::new();
        for (transaction, values) in hydrated.iter().zip(entry_values.iter()) {
            payloads.push(crate::outbox::OutboxEventPayload::TransactionCreated {
                transaction: transaction.values().clone(),
            });
            payloads.extend(values.iter().map(|entry| {
                crate::outbox::OutboxEventPayload::EntryCreated {
                    entry: entry.clone(),
                }
            }));
        }
        self.publisher.publish_all(db, payloads.into_iter()).await?;

        Ok(hydrated)
    }

    // ------------------------------------------------------------------
    // preparation — template evaluation plus batch-level checks
    // ------------------------------------------------------------------

    fn prepare_one(
        &self,
        input: &PostingInput,
        templates: &HashMap<String, ResolvedTemplate>,
        posting: PostingRef,
    ) -> Result<PreparedTransaction, PreparePostingRejection> {
        let template = templates
            .get(&input.tx_template_code)
            .expect("template resolved above");
        posting.record_failure(self.tx_templates.prepare_transaction(
            posting,
            &template.values,
            input.params.clone(),
        ))
    }

    fn prepare_all(
        &self,
        batch: &[PostingInput],
        templates: &HashMap<String, ResolvedTemplate>,
    ) -> Result<Vec<PreparedTransaction>, BatchPreparePostingRejection> {
        let mut prepared = Vec::with_capacity(batch.len());
        let mut seen_ids = HashSet::new();
        let mut seen_external = HashSet::new();
        for (index, input) in batch.iter().enumerate() {
            let posting = PostingRef {
                index,
                tx_id: input.tx_id,
            };
            let transaction = self.prepare_one(input, templates, posting)?;
            if !seen_ids.insert(transaction.tx_id) {
                return Err(
                    BatchPreparePostingRejection::DuplicateTransactionIdInBatch {
                        posting: posting.record(),
                        tx_id: transaction.tx_id,
                    },
                );
            }
            if let Some(external_id) = transaction.external_id.as_ref() {
                if !seen_external.insert(external_id.clone()) {
                    return Err(BatchPreparePostingRejection::DuplicateExternalIdInBatch {
                        posting: posting.record(),
                        external_id: external_id.clone(),
                    });
                }
            }
            prepared.push(transaction);
        }
        Ok(prepared)
    }

    fn validate_one(
        posting: &PreparedTransaction,
        read: &PostingState,
        attribution: PostingRef,
    ) -> Result<(), ValidatePostingRejection> {
        match read.journals.get(&posting.journal_id) {
            None => {
                return Err(ValidatePostingRejection::JournalNotFound {
                    posting: attribution.record(),
                    journal_id: posting.journal_id,
                })
            }
            Some(journal) if journal.status == Status::Locked => {
                return Err(ValidatePostingRejection::JournalLocked {
                    posting: attribution.record(),
                    journal_id: posting.journal_id,
                })
            }
            Some(_) => {}
        }
        for entry in &posting.entries {
            let account_id = entry.account_id();
            let Some(meta) = read.accounts.get(&account_id) else {
                return Err(ValidatePostingRejection::AccountNotFound {
                    posting: attribution.record(),
                    account_id,
                });
            };
            if meta.is_account_set {
                return Err(ValidatePostingRejection::EntryTargetsAccountSet {
                    posting: attribution.record(),
                    account_id,
                });
            }
            if meta.locked {
                return Err(ValidatePostingRejection::AccountLocked {
                    posting: attribution.record(),
                    account_id,
                });
            }
        }
        Ok(())
    }

    fn validate_batch(
        &self,
        batch: &[PostingInput],
        prepared: &[PreparedTransaction],
        read: &PostingState,
    ) -> Result<(), ValidatePostingRejection> {
        for (index, prepared) in prepared.iter().enumerate() {
            Self::validate_one(
                prepared,
                read,
                PostingRef {
                    index,
                    tx_id: batch[index].tx_id,
                },
            )?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // ancestor resolution
    // ------------------------------------------------------------------

    /// Expand the probed memberships into ancestor mappings, take the ancestor
    /// lock batch, and merge the supplemental read into `read`.
    ///
    /// Journals are processed in ascending id order. Within a journal the
    /// ancestor lock batch is already canonically sorted; iterating journals in
    /// a fixed order extends that canonical order across a batch that spans
    /// journals, so two concurrent multi-journal batches cannot acquire the
    /// same ancestor locks in opposite orders.
    ///
    /// **The result is keyed by journal, and must stay that way.** A leaf
    /// account carries no journal of its own, so the same leaf may be a member
    /// of sets in several journals. Flattening the per-journal results into one
    /// `leaf -> sets` map lets a posting in journal A fan into a set belonging
    /// to journal B: the fold takes the set id from the map but the `journal_id`
    /// from the *entry*, writing a balance row for (journal A, set B) that no
    /// lock covers and that no query should ever see. Velocity has the same
    /// hazard via `VelocityBalanceKey`.
    async fn resolve_ancestors(
        &self,
        db: &mut impl AtomicOperation,
        prepared: &[PreparedTransaction],
        read: &mut PostingState,
    ) -> Result<AncestorMappings, Fail<BalanceAccountLocked, lanes!(Transient, Fatal)>> {
        let mut mappings: AncestorMappings = HashMap::new();
        if read.seeds.is_empty() {
            return Ok(mappings);
        }

        let mut journals: Vec<JournalId> = Self::dedup(prepared.iter().map(|p| p.journal_id));
        journals.sort_unstable();

        let mut ancestor_keys = BalanceKeys::default();
        let mut ancestor_ids: Vec<AccountId> = Vec::new();
        for journal_id in journals {
            let entry_pairs: (Vec<AccountId>, Vec<&str>) = prepared
                .iter()
                .filter(|p| p.journal_id == journal_id)
                .flat_map(|p| p.entries.iter())
                .map(|e| (e.account_id(), e.currency().code()))
                .collect::<HashSet<_>>()
                .into_iter()
                .unzip();
            if entry_pairs.0.is_empty() {
                continue;
            }

            let resolved = self
                .account_sets
                .resolve_mappings_from_probe_in_op(
                    db,
                    journal_id,
                    read.epoch,
                    &read.seeds,
                    &entry_pairs,
                )
                .await?;

            // The ancestor rows this journal's postings will write: each leaf's
            // currencies propagate to exactly its own ancestors.
            for posting in prepared.iter().filter(|p| p.journal_id == journal_id) {
                for entry in posting.entries.iter() {
                    for set_id in resolved.get(&entry.account_id()).into_iter().flatten() {
                        let account_id = AccountId::from(set_id);
                        ancestor_keys.push(journal_id, account_id, entry.currency());
                        ancestor_ids.push(account_id);
                    }
                }
            }
            let per_journal = mappings.entry(journal_id).or_default();
            for (account_id, sets) in resolved {
                per_journal.entry(account_id).or_default().extend(sets);
            }
        }

        if ancestor_keys.is_empty() {
            return Ok(mappings);
        }

        ancestor_ids.sort_unstable();
        ancestor_ids.dedup();
        let supplemental = self
            .repo
            .read_ancestor_state_in_op(db, &ancestor_ids, &ancestor_keys.sorted_deduped())
            .await?;
        read.accounts.extend(supplemental.accounts);
        read.balances.extend(supplemental.balances);
        read.controls.extend(supplemental.controls);

        // Ancestor sets are subject to the same locked-account rejection as
        // entry accounts. Locked is locked, regardless of how the ancestor's
        // balances are maintained.
        for account_id in ancestor_ids {
            if let Some(meta) = read.accounts.get(&account_id) {
                if meta.locked {
                    return Err(BalanceAccountLocked(account_id).into());
                }
            }
        }
        Ok(mappings)
    }

    // ------------------------------------------------------------------
    // the fold
    // ------------------------------------------------------------------

    /// Chain every posting's entry deltas into balance snapshots, per journal.
    ///
    /// The involved set is the keys present in `current`: a pair that was not
    /// loaded is not folded. That is what keeps eventually-consistent accounts
    /// (whose balances the streaming rollup owns) and any pair outside this
    /// flow out of the inline write, and it is the same filter the
    /// pre-consolidation balance read applied.
    fn fold_balances(
        &self,
        transactions: &[Transaction],
        entry_values: &[Vec<EntryValues>],
        read: &PostingState,
        mappings: &AncestorMappings,
        now: DateTime<Utc>,
    ) -> Vec<BalanceSnapshot> {
        let mut journals: Vec<JournalId> =
            Self::dedup(transactions.iter().map(|tx| tx.values().journal_id));
        journals.sort_unstable();

        let empty = HashMap::new();
        let mut all = Vec::new();
        for journal_id in journals {
            // Only this journal's ancestor sets; see `resolve_ancestors`.
            let mappings = mappings.get(&journal_id).unwrap_or(&empty);
            let entries: Vec<EntryValues> = transactions
                .iter()
                .zip(entry_values)
                .filter(|(tx, _)| tx.values().journal_id == journal_id)
                .flat_map(|(_, values)| values.iter().cloned())
                .collect();
            if entries.is_empty() {
                continue;
            }

            let mut current: HashMap<(AccountId, Currency), Option<BalanceSnapshot>> =
                HashMap::new();
            for entry in entries.iter() {
                for account_id in mappings
                    .get(&entry.account_id)
                    .into_iter()
                    .flatten()
                    .map(AccountId::from)
                    .chain(std::iter::once(entry.account_id))
                {
                    // Only pairs the flow actually loaded participate; an
                    // eventually-consistent account has no `accounts` entry
                    // marking it non-EC, so it is skipped here.
                    let involved = read
                        .accounts
                        .get(&account_id)
                        .is_some_and(|meta| !meta.eventually_consistent);
                    if !involved {
                        continue;
                    }
                    current
                        .entry((account_id, entry.currency))
                        .or_insert_with(|| {
                            read.balances
                                .get(&(journal_id, account_id, entry.currency))
                                .cloned()
                        });
                }
            }

            all.extend(crate::balance::Snapshots::from_entries(
                now, current, &entries, mappings,
            ));
        }
        all
    }

    // ------------------------------------------------------------------
    // velocity + effective balances
    // ------------------------------------------------------------------

    /// Maintain cumulative-effective balances for the batch, one pass per
    /// `(journal, effective date)` group.
    ///
    /// Each group's entries are folded in memory into one delta per
    /// `(account, currency)` at the group's date, and applied by a set-based
    /// statement pair that adds the delta to the date's row (creating it from
    /// its predecessor if needed) and to every later row of the pair — see
    /// `EffectiveBalanceRepo::apply_deltas_in_op`. No stored balance row is
    /// loaded into the application, however far back the posting is dated.
    ///
    /// Groups run in ascending date order so a batch spanning dates behaves like
    /// the same postings submitted oldest-first. In the overwhelmingly common
    /// case — every posting on today's date, one journal — this is a single
    /// pass for the whole batch.
    async fn update_effective_balances(
        &self,
        db: &mut impl AtomicOperation,
        transactions: &[Transaction],
        entry_values: &[Vec<EntryValues>],
        read: &PostingState,
        mappings: &AncestorMappings,
        now: DateTime<Utc>,
    ) -> Result<(), CalaFault> {
        let mut groups: Vec<((JournalId, chrono::NaiveDate), Vec<EntryValues>)> = Vec::new();
        for (transaction, entries) in transactions.iter().zip(entry_values) {
            let journal_id = transaction.values().journal_id;
            let enabled = read
                .journals
                .get(&journal_id)
                .is_some_and(|j| j.config.enable_effective_balances);
            if !enabled {
                continue;
            }
            let key = (journal_id, transaction.values().effective);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, group)) => group.extend(entries.iter().cloned()),
                None => groups.push((key, entries.clone())),
            }
        }
        groups.sort_by_key(|((journal_id, effective), _)| (*journal_id, *effective));

        let empty = HashMap::new();
        for ((journal_id, effective), entries) in groups {
            // Only this journal's ancestor sets; see `resolve_ancestors`.
            let mappings = mappings.get(&journal_id).unwrap_or(&empty);
            let involved: HashSet<(AccountId, Currency)> = entries
                .iter()
                .flat_map(|entry| {
                    mappings
                        .get(&entry.account_id)
                        .into_iter()
                        .flatten()
                        .map(AccountId::from)
                        .chain(std::iter::once(entry.account_id))
                        .map(move |id| (id, entry.currency))
                })
                .filter(|(id, _)| {
                    read.accounts
                        .get(id)
                        .is_some_and(|meta| !meta.eventually_consistent)
                })
                .collect();
            if involved.is_empty() {
                continue;
            }
            self.balances
                .effective()
                .update_cumulative_balances_in_op(
                    db,
                    journal_id,
                    entries,
                    effective,
                    now,
                    mappings.clone(),
                    involved,
                )
                .await?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // small helpers
    // ------------------------------------------------------------------

    fn dedup<T: Ord + Clone>(items: impl Iterator<Item = T>) -> Vec<T> {
        let mut out: Vec<T> = items.collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The distinct `(journal, account, currency)` triples the flow locks and
    /// reads, in canonical acquisition order.
    fn entry_balance_keys(prepared: &[PreparedTransaction]) -> BalanceKeys {
        let mut keys = BalanceKeys::default();
        for posting in prepared {
            for entry in posting.entries.iter() {
                keys.push(posting.journal_id, entry.account_id(), entry.currency());
            }
        }
        keys.sorted_deduped()
    }
}
