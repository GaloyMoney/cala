//! The posting flow: transactions in, rows out, in a fixed number of phases.
//!
//! # Shape
//!
//! Every statement takes arrays, so posting one transaction is the N=1 case of
//! posting a batch. The simple path — no account-set membership, no velocity
//! controls, no effective balances — costs **six round trips regardless of
//! batch size**:
//!
//! | # | statement | phase |
//! |---|-----------|-------|
//! | 1 | `BEGIN` | |
//! | 2 | fence: union advisory locks + `now()` + template versions | [`repo::fence_in_op`] |
//! | 3 | read: memberships + epoch + journals + account meta + velocity controls + balances | [`repo::read_in_op`] |
//! | 4 | apply: transactions + entries + both event streams + balance snapshots | [`repo::apply_in_op`] |
//! | 5 | outbox insert (obix commit hook) | |
//! | 6 | `COMMIT` | |
//!
//! Between 3 and 4 the flow runs entirely in memory: ancestor expansion against
//! the set-graph cache, the chained balance fold, and velocity enforcement.
//! Nothing is written until all of it has succeeded, which is what lets a
//! rejection name the posting that caused it with no rows to undo.
//!
//! Features that a deployment actually uses cost extra statements, and only
//! then: non-EC ancestor sets add a lock statement and a supplemental read (one
//! pair per journal involved); velocity controls that match add their existing
//! read/write per posting; a journal with effective balances adds its existing
//! read/write per posting.
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

mod error;
mod prepare;
mod repo;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use es_entity::AtomicOperation;
use sqlx::PgPool;
use tracing::instrument;

use cala_types::{
    balance::BalanceSnapshot, entry::EntryValues, velocity::VelocityContextAccountValues,
};

use crate::{
    account_set::AccountSets,
    balance::Balances,
    entry::{Entry, NewEntry},
    ledger::error::LedgerError,
    outbox::OutboxPublisher,
    primitives::*,
    transaction::{NewTransaction, Transaction},
    tx_template::Params,
    velocity::{AccountVelocityControl, Velocities},
};

pub use error::{PostingError, PostingErrorKind};

use prepare::{PreparedPosting, ResolvedTemplate, TemplateCache};
use repo::*;

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
/// ancestor resolution, velocity enforcement, effective-balance maintenance —
/// but issues the transaction/entry/balance statements itself, because fusing
/// them is the point.
#[derive(Clone)]
pub struct Postings {
    account_sets: AccountSets,
    balances: Balances,
    velocities: Velocities,
    publisher: OutboxPublisher,
    clock: es_entity::clock::ClockHandle,
    templates: TemplateCache,
    _pool: PgPool,
}

impl Postings {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        clock: &es_entity::clock::ClockHandle,
        account_sets: &AccountSets,
        balances: &Balances,
        velocities: &Velocities,
    ) -> Self {
        Self {
            account_sets: account_sets.clone(),
            balances: balances.clone(),
            velocities: velocities.clone(),
            publisher: publisher.clone(),
            clock: clock.clone(),
            templates: TemplateCache::default(),
            _pool: pool.clone(),
        }
    }

    /// Post a batch of transactions in one operation, all-or-nothing.
    ///
    /// Any failure aborts the whole batch; the error names the offending
    /// posting. See the module docs for the ordering guarantees within a batch.
    #[instrument(
        name = "cala_ledger.posting.post_all_in_op",
        skip_all,
        fields(
            batch_size = batch.len(),
            failed_posting_index = tracing::field::Empty,
            failed_posting_id = tracing::field::Empty,
        ),
        err(level = "warn")
    )]
    pub(crate) async fn post_all_in_op(
        &self,
        db: &mut impl AtomicOperation,
        batch: Vec<PostingInput>,
    ) -> Result<Vec<Transaction>, LedgerError> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        self.post_all_inner(db, batch)
            .await
            .map_err(LedgerError::from)
    }

    async fn post_all_inner(
        &self,
        db: &mut impl AtomicOperation,
        batch: Vec<PostingInput>,
    ) -> Result<Vec<Transaction>, Flow> {
        // ---- prepare (client-side) ------------------------------------
        let codes: Vec<String> = dedup(batch.iter().map(|p| p.tx_template_code.clone()));
        let used = self.resolve_templates(db, &codes).await?;
        let mut prepared = self.prepare_all(&batch, &used)?;

        // ---- phase 1: fence -------------------------------------------
        let mut keys = entry_balance_keys(&prepared);
        let fence = fence_in_op(db, &keys, &codes, db.maybe_now()).await?;

        // A template version moved between preparation and the fence. Nothing
        // has been written, so re-resolving and re-preparing is safe; any entry
        // pair the new bodies introduced needs its lock, which is taken in a
        // supplemental sorted batch.
        //
        // Deadlock note: those supplemental locks are in the same key class as
        // the ones already held, so acquiring them out of the flow's canonical
        // order opens a window against a concurrent poster. It requires a
        // template update landing between two statements of a live posting, and
        // Postgres resolves it by aborting one side with a retryable deadlock
        // error rather than hanging.
        let stale = TemplateCache::check(&used, &fence.templates);
        if !stale.is_empty() {
            let refreshed = self.resolve_templates(db, &stale).await?;
            let mut merged = used;
            merged.extend(refreshed);
            prepared = self.prepare_all(&batch, &merged)?;
            let new_keys = entry_balance_keys(&prepared);
            fence_in_op(db, &new_keys, &[], db.maybe_now()).await?;
            keys = new_keys;
        }

        let now = fence.now;

        // ---- phase 2: read --------------------------------------------
        let account_ids = dedup(
            prepared
                .iter()
                .flat_map(|p| p.entries.iter().map(|e| e.account_id())),
        );
        let journal_ids = dedup(prepared.iter().map(|p| p.journal_id));
        let mut read = read_in_op(db, &account_ids, &journal_ids, &keys).await?;

        self.validate(&batch, &prepared, &read)?;

        // ---- ancestor phase (only when memberships exist) --------------
        let mappings = self.resolve_ancestors(db, &prepared, &mut read).await?;

        // ---- fold + enforce (client-side) ------------------------------
        let (transactions, entries_per_posting) = prepared
            .into_iter()
            .map(|p| p.into_new_transaction(now))
            .collect::<(Vec<_>, Vec<_>)>();

        let mut hydrated = Vec::with_capacity(transactions.len());
        let mut entry_values: Vec<Vec<EntryValues>> = Vec::with_capacity(transactions.len());
        let mut tx_rows = TransactionRows::default();
        let mut tx_events = EventRows::default();
        let mut entry_rows = EntryRows::default();
        let mut entry_events = EventRows::default();

        for (new_tx, new_entries) in transactions.into_iter().zip(entries_per_posting) {
            let (transaction, tx_event_rows) = build_transaction(new_tx, now);
            push_transaction(&mut tx_rows, &mut tx_events, &transaction, tx_event_rows);

            let mut values = Vec::with_capacity(new_entries.len());
            for new_entry in new_entries {
                let (entry, event_row) = build_entry(new_entry, now);
                push_entry(&mut entry_rows, &mut entry_events, &entry, event_row);
                values.push(entry.into_values());
            }
            entry_values.push(values);
            hydrated.push(transaction);
        }

        let snapshots = self.fold_balances(&hydrated, &entry_values, &read, &mappings, now);

        // Velocity and effective balances keep their existing per-posting
        // shape: both are driven by a single `TransactionValues` (velocity's
        // evaluation context, effective balances' back-dating replay). Within
        // one database transaction each posting's read still observes its
        // predecessors' writes, so batch ordering is preserved.
        for (transaction, values) in hydrated.iter().zip(entry_values.iter()) {
            self.enforce_velocity(db, transaction, values, &read, &mappings, now)
                .await?;
        }

        // ---- phase 3: apply --------------------------------------------
        apply_in_op(
            db,
            now,
            &tx_rows,
            &tx_events,
            &entry_rows,
            &entry_events,
            &snapshots,
        )
        .await?;

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
        self.publisher
            .publish_all(db, payloads.into_iter())
            .await
            .map_err(|e| Flow::Ledger(e.into()))?;

        Ok(hydrated)
    }

    // ------------------------------------------------------------------
    // template resolution
    // ------------------------------------------------------------------

    async fn resolve_templates(
        &self,
        db: &mut impl AtomicOperation,
        codes: &[String],
    ) -> Result<HashMap<String, ResolvedTemplate>, Flow> {
        let mut used = HashMap::new();
        let mut missing = Vec::new();
        for code in codes {
            match self.templates.get(code) {
                Some(resolved) => {
                    used.insert(code.clone(), resolved);
                }
                None => missing.push(code.clone()),
            }
        }
        if missing.is_empty() {
            return Ok(used);
        }

        let fetched = resolve_templates_in_op(db, &missing).await?;
        for code in missing {
            let Some((id, version, event)) = fetched.get(&code) else {
                return Err(Flow::Ledger(
                    crate::tx_template::error::TxTemplateError::NotFound.into(),
                ));
            };
            let event: crate::tx_template::TxTemplateEvent = serde_json::from_value(event.clone())
                .map_err(|e| {
                    Flow::Ledger(crate::tx_template::error::TxTemplateError::from(e).into())
                })?;
            let resolved = ResolvedTemplate {
                id: *id,
                version: *version,
                values: Arc::new(event.into_values()),
            };
            self.templates.insert(&code, resolved.clone());
            used.insert(code, resolved);
        }
        Ok(used)
    }

    fn prepare_all(
        &self,
        batch: &[PostingInput],
        templates: &HashMap<String, ResolvedTemplate>,
    ) -> Result<Vec<PreparedPosting>, Flow> {
        let mut prepared = Vec::with_capacity(batch.len());
        let mut seen_ids = HashSet::new();
        let mut seen_external = HashSet::new();
        for (index, input) in batch.iter().enumerate() {
            let template = templates
                .get(&input.tx_template_code)
                .expect("template resolved above");
            let posting = prepare::prepare(
                &self.clock,
                &template.values,
                input.tx_id,
                input.params.clone(),
            )
            .map_err(|e| Flow::posting(index, input.tx_id, e))?;

            if !seen_ids.insert(posting.tx_id) {
                return Err(Flow::posting(
                    index,
                    input.tx_id,
                    PostingErrorKind::DuplicateTransactionIdInBatch(posting.tx_id),
                ));
            }
            if let Some(external_id) = posting.external_id.as_ref() {
                if !seen_external.insert(external_id.clone()) {
                    return Err(Flow::posting(
                        index,
                        input.tx_id,
                        PostingErrorKind::DuplicateExternalIdInBatch(external_id.clone()),
                    ));
                }
            }
            prepared.push(posting);
        }
        Ok(prepared)
    }

    // ------------------------------------------------------------------
    // validation — everything that can reject a posting, before any write
    // ------------------------------------------------------------------

    fn validate(
        &self,
        batch: &[PostingInput],
        prepared: &[PreparedPosting],
        read: &ReadOutcome,
    ) -> Result<(), Flow> {
        for (index, posting) in prepared.iter().enumerate() {
            let tx_id = batch[index].tx_id;

            match read.journals.get(&posting.journal_id) {
                None => {
                    return Err(Flow::posting(
                        index,
                        tx_id,
                        PostingErrorKind::JournalNotFound(posting.journal_id),
                    ))
                }
                Some(journal) if journal.status == Status::Locked => {
                    return Err(Flow::posting(
                        index,
                        tx_id,
                        PostingErrorKind::JournalLocked(posting.journal_id),
                    ))
                }
                Some(_) => {}
            }

            for entry in posting.entries.iter() {
                let account_id = entry.account_id();
                let Some(meta) = read.accounts.get(&account_id) else {
                    return Err(Flow::posting(
                        index,
                        tx_id,
                        PostingErrorKind::AccountNotFound(account_id),
                    ));
                };
                if meta.is_account_set {
                    return Err(Flow::posting(
                        index,
                        tx_id,
                        PostingErrorKind::EntryTargetsAccountSet(account_id),
                    ));
                }
                // Status is checked only for accounts the poster writes
                // balances for. Eventually-consistent leaves are excluded from
                // the inline path (the streaming rollup owns their balances),
                // so their status was not checked before consolidation either.
                if meta.locked && !meta.eventually_consistent {
                    return Err(Flow::posting(
                        index,
                        tx_id,
                        PostingErrorKind::AccountLocked(account_id),
                    ));
                }
            }
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
    async fn resolve_ancestors(
        &self,
        db: &mut impl AtomicOperation,
        prepared: &[PreparedPosting],
        read: &mut ReadOutcome,
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, Flow> {
        let mut mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        if read.seeds.is_empty() {
            return Ok(mappings);
        }

        let mut journals: Vec<JournalId> = dedup(prepared.iter().map(|p| p.journal_id));
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
                .await
                .map_err(|e| Flow::Ledger(e.into()))?;

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
            for (account_id, sets) in resolved {
                mappings.entry(account_id).or_default().extend(sets);
            }
        }

        if ancestor_keys.is_empty() {
            return Ok(mappings);
        }

        ancestor_ids.sort_unstable();
        ancestor_ids.dedup();
        let supplemental =
            read_ancestors_in_op(db, &ancestor_ids, &ancestor_keys.sorted_deduped()).await?;
        read.accounts.extend(supplemental.accounts);
        read.balances.extend(supplemental.balances);
        read.controls.extend(supplemental.controls);

        // Ancestor sets are subject to the same locked-account rejection as
        // entry accounts (the pre-consolidation balance read raised it for
        // every non-EC row it loaded, ancestors included).
        for account_id in ancestor_ids {
            if let Some(meta) = read.accounts.get(&account_id) {
                if meta.locked && !meta.eventually_consistent {
                    return Err(Flow::Ledger(LedgerError::BalanceError(
                        crate::balance::error::BalanceError::AccountLocked(account_id),
                    )));
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
        read: &ReadOutcome,
        mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        now: DateTime<Utc>,
    ) -> Vec<BalanceSnapshot> {
        let mut journals: Vec<JournalId> =
            dedup(transactions.iter().map(|tx| tx.values().journal_id));
        journals.sort_unstable();

        let mut all = Vec::new();
        for journal_id in journals {
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

    async fn enforce_velocity(
        &self,
        db: &mut impl AtomicOperation,
        transaction: &Transaction,
        entries: &[EntryValues],
        read: &ReadOutcome,
        mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        now: DateTime<Utc>,
    ) -> Result<(), Flow> {
        if read.controls.is_empty() {
            return Ok(());
        }
        let controls: HashMap<
            AccountId,
            (VelocityContextAccountValues, Vec<AccountVelocityControl>),
        > = entries
            .iter()
            .flat_map(|entry| {
                mappings
                    .get(&entry.account_id)
                    .into_iter()
                    .flatten()
                    .map(AccountId::from)
                    .chain(std::iter::once(entry.account_id))
            })
            .filter_map(|id| read.controls.get(&id).map(|v| (id, v.clone())))
            .collect();
        if controls.is_empty() {
            return Ok(());
        }
        self.velocities
            .enforce_with_controls_in_op(db, now, transaction.values(), entries, controls, mappings)
            .await
            .map_err(|e| Flow::Ledger(e.into()))
    }

    async fn update_effective_balances(
        &self,
        db: &mut impl AtomicOperation,
        transactions: &[Transaction],
        entry_values: &[Vec<EntryValues>],
        read: &ReadOutcome,
        mappings: &HashMap<AccountId, Vec<AccountSetId>>,
        now: DateTime<Utc>,
    ) -> Result<(), Flow> {
        for (transaction, entries) in transactions.iter().zip(entry_values) {
            let journal_id = transaction.values().journal_id;
            let enabled = read
                .journals
                .get(&journal_id)
                .is_some_and(|j| j.config.enable_effective_balances);
            if !enabled {
                continue;
            }
            let involved: (Vec<AccountId>, Vec<&str>) = entries
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
                .collect::<HashSet<_>>()
                .into_iter()
                .map(|(id, currency)| (id, currency.code()))
                .unzip();
            if involved.0.is_empty() {
                continue;
            }
            self.balances
                .effective()
                .update_cumulative_balances_in_op(
                    db,
                    journal_id,
                    entries.clone(),
                    transaction.values().effective,
                    now,
                    mappings.clone(),
                    involved,
                )
                .await
                .map_err(|e| Flow::Ledger(e.into()))?;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------
// helpers
// ----------------------------------------------------------------------

/// A failure anywhere in the flow: either attributable to one posting or not.
enum Flow {
    Posting(PostingError),
    Ledger(LedgerError),
}

impl Flow {
    fn posting(index: usize, tx_id: TransactionId, kind: impl Into<PostingErrorKind>) -> Self {
        Self::Posting(PostingError::at(index, tx_id, kind))
    }
}

impl From<sqlx::Error> for Flow {
    fn from(e: sqlx::Error) -> Self {
        Self::Ledger(e.into())
    }
}

impl From<Flow> for LedgerError {
    fn from(flow: Flow) -> Self {
        match flow {
            Flow::Ledger(e) => e,
            Flow::Posting(e) => repo::to_ledger_error(e),
        }
    }
}

fn dedup<T: Ord + Clone>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut out: Vec<T> = items.collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The distinct `(journal, account, currency)` triples the flow locks and
/// reads, in canonical acquisition order.
fn entry_balance_keys(prepared: &[PreparedPosting]) -> BalanceKeys {
    let mut keys = BalanceKeys::default();
    for posting in prepared {
        for entry in posting.entries.iter() {
            keys.push(posting.journal_id, entry.account_id(), entry.currency());
        }
    }
    keys.sorted_deduped()
}

/// Hydrate a transaction and produce the event row the apply statement writes.
fn build_transaction(
    new_tx: NewTransaction,
    now: DateTime<Utc>,
) -> (Transaction, (i32, String, serde_json::Value)) {
    use es_entity::{IntoEvents, TryFromEvents};
    let mut events = new_tx.into_events();
    let types = events.new_event_types();
    let serialized = events.serialize_new_events();
    events.mark_new_events_persisted_at(now);
    let entity = Transaction::try_from_events(events).expect("transaction hydration");
    (
        entity,
        (
            1,
            types.into_iter().next().expect("one initial event"),
            serialized.into_iter().next().expect("one initial event"),
        ),
    )
}

fn build_entry(
    new_entry: NewEntry,
    now: DateTime<Utc>,
) -> (Entry, (i32, String, serde_json::Value)) {
    use es_entity::{IntoEvents, TryFromEvents};
    let mut events = new_entry.into_events();
    let types = events.new_event_types();
    let serialized = events.serialize_new_events();
    events.mark_new_events_persisted_at(now);
    let entity = Entry::try_from_events(events).expect("entry hydration");
    (
        entity,
        (
            1,
            types.into_iter().next().expect("one initial event"),
            serialized.into_iter().next().expect("one initial event"),
        ),
    )
}

fn push_transaction(
    rows: &mut TransactionRows,
    events: &mut EventRows,
    transaction: &Transaction,
    (sequence, event_type, event): (i32, String, serde_json::Value),
) {
    let values = transaction.values();
    rows.ids.push(values.id);
    rows.journal_ids.push(values.journal_id);
    rows.template_ids.push(values.tx_template_id);
    rows.external_ids.push(values.external_id.clone());
    rows.correlation_ids.push(values.correlation_id.clone());
    rows.effectives.push(values.effective);
    events.ids.push(values.id.into());
    events.sequences.push(sequence);
    events.event_types.push(event_type);
    events.events.push(event);
}

fn push_entry(
    rows: &mut EntryRows,
    events: &mut EventRows,
    entry: &Entry,
    (sequence, event_type, event): (i32, String, serde_json::Value),
) {
    let values = entry.values();
    rows.ids.push(values.id);
    rows.journal_ids.push(values.journal_id);
    rows.account_ids.push(values.account_id);
    rows.transaction_ids.push(values.transaction_id);
    events.ids.push(values.id.into());
    events.sequences.push(sequence);
    events.event_types.push(event_type);
    events.events.push(event);
}
