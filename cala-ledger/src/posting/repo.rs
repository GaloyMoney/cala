//! The posting flow's SQL — three statements on the simple path.
//!
//! # Why the phases are separate statements
//!
//! The lock/read/write split is **not** stylistic; each boundary is a
//! correctness requirement inherited from the pre-consolidation path, and
//! collapsing any of them reintroduces a race:
//!
//! - **lock -> read.** An advisory-lock wait *inside* a statement does not
//!   refresh that statement's snapshot (the snapshot is taken at statement
//!   start). Any read the locks protect — the balance rows, and above
//!   all the account-set memberships — must therefore run in a *later*
//!   statement, or a concurrent attach committing while we block on the locks
//!   would be invisible to us. This is precisely the stale-read class the
//!   attach fence exists to close.
//! - **read -> write.** Balance snapshot versioning and velocity limit
//!   enforcement run in Rust between them. Nothing is written until every
//!   posting has been folded and enforced, which is what makes a rejection
//!   attributable to a single posting with no rows to undo.
//!
//! # Lock doctrine (carried over verbatim in force from `balance::repo`)
//!
//! Class-1 advisory locks (`EC_SET_LOCK_CLASS`): SHARED = the poster, on its
//! distinct **entry accounts** (leaves only, EC and non-EC alike — never
//! ancestors), held from *before its first entry insert* to commit — which
//! [`PostingRepo::lock_balances_and_probe_templates_in_op`] satisfies by
//! construction, since it is the flow's first statement and the entry rows are
//! written by [`PostingRepo::insert_postings_and_balances_in_op`], the last.
//! EXCLUSIVE = the membership guard on the member being added/removed
//! (`BalanceRepo::member_has_balance_history_in_op`); the streaming rollup
//! applier takes SHARED on the EC accounts it writes. The create-inside-set
//! fast path (`NewAccount::initial_account_set`) is exempt from the fence:
//! its account is uncommitted, so no history exists and no first posting can
//! be in flight (see the field docs).
//!
//! Per-balance FOR_UPDATE locks (1-arg `pg_advisory_xact_lock`, keyed on
//! `(journal_id, account_id, currency)`) are the poster-vs-poster serializer
//! for the balance rows the posting writes inline. EC pairs take none — posters
//! never write `cala_current_balances` rows for EC accounts. Non-EC *ancestor*
//! pairs are unknowable before the membership read, so they are acquired in a
//! second phase (`AccountSetRepo::lock_resolved_ancestors_in_op`), which reads
//! only the membership graph — never balance values — so lock-before-read still
//! holds across statements.
//!
//! **Lock-ordering invariant.** The `ORDER BY` in
//! [`PostingRepo::lock_balances_and_probe_templates_in_op`] is what makes
//! acquisition order canonical — do not remove it. It is *required* because
//! the JOIN lets the planner produce
//! rows in arbitrary order (a hash join probing from a `cala_accounts` scan
//! emits heap order, not UNNEST array order — observed as real deadlocks and
//! fixed by adding the `ORDER BY`). It is also *sufficient*: advisory-lock
//! functions are VOLATILE, and since PostgreSQL 9.6 the planner
//! unconditionally postpones volatile SELECT-list expressions until after the
//! Sort (`make_sort_input_target` — the same mechanism that makes `nextval()`
//! follow `ORDER BY`), so the lock calls run row by row in sorted order. The
//! CTE is `MATERIALIZED` so that this holds regardless of how the outer query
//! consumes it. Duplicate pairs re-acquire the same locks — a no-op within one
//! transaction.
//!
//! **This is also what makes batching safe.** A batch takes ONE sorted, deduped
//! union fence covering every posting's entry pairs, then ONE sorted ancestor
//! batch — two phases over disjoint key classes (entry accounts are never
//! account sets, per the set-guard FK), in the same order for every poster. It
//! is the per-posting lock acquisition of the old loop — locks taken across
//! posting boundaries with no global ordering — that could deadlock concurrent
//! batches, not batching itself.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use cala_types::{
    balance::BalanceSnapshot, journal::JournalValues, velocity::VelocityContextAccountValues,
};

use crate::{
    account_set::AccountMembership,
    entry::{Entry, NewEntry},
    primitives::*,
    transaction::{NewTransaction, Transaction},
    velocity::AccountVelocityControl,
};

/// Advisory-lock class shared with `balance::repo` — the attach fence's
/// namespace. Must stay in sync with `BalanceRepo::EC_SET_LOCK_CLASS`.
const EC_SET_LOCK_CLASS: i32 = 1;

/// Maximum balance snapshots written per `INSERT` in
/// [`PostingRepo::insert_postings_and_balances_in_op`]. A large batch fanning
/// into deep ancestor chains would otherwise become one multi-million-row
/// statement big enough to
/// OOM a Postgres backend. Each sub-batch is self-contained (history insert +
/// current upsert), so the balance-history FK is satisfied per sub-batch; the
/// whole set still commits atomically as one transaction.
const INSERT_SNAPSHOT_BATCH_SIZE: usize = 5_000;

/// The `(journal, account, currency)` triples a flow locks and reads.
#[derive(Default)]
pub(super) struct BalanceKeys {
    pub journal_ids: Vec<JournalId>,
    pub account_ids: Vec<AccountId>,
    pub currencies: Vec<&'static str>,
}

impl BalanceKeys {
    pub(super) fn is_empty(&self) -> bool {
        self.account_ids.is_empty()
    }

    pub(super) fn push(
        &mut self,
        journal_id: JournalId,
        account_id: AccountId,
        currency: Currency,
    ) {
        self.journal_ids.push(journal_id);
        self.account_ids.push(account_id);
        self.currencies.push(currency.code());
    }

    /// Deduped and sorted into the canonical acquisition order. Sorting here
    /// rather than relying on input order keeps the SQL's `ORDER BY` and the
    /// Rust-side order in agreement for every caller.
    pub(super) fn sorted_deduped(mut self) -> Self {
        let mut triples: Vec<_> = self
            .account_ids
            .drain(..)
            .zip(self.currencies.drain(..))
            .zip(self.journal_ids.drain(..))
            .map(|((a, c), j)| (a, c, j))
            .collect();
        triples.sort_unstable();
        triples.dedup();
        let mut out = Self::default();
        for (account_id, currency, journal_id) in triples {
            out.account_ids.push(account_id);
            out.currencies.push(currency);
            out.journal_ids.push(journal_id);
        }
        out
    }
}

/// What the locking statement returns besides the locks it took.
pub(super) struct LockOutcome {
    /// The transaction timestamp — `transaction_timestamp()`, pinned at
    /// `BEGIN`, so it is byte-identical to the dedicated `SELECT NOW()` the
    /// pre-consolidation path issued, and shared by every row the flow writes.
    pub now: DateTime<Utc>,
    /// `code -> (id, latest version)` as committed right now, for the codes
    /// the flow prepared against.
    pub template_versions: HashMap<String, (TxTemplateId, i32)>,
}

/// Immutable per-account facts the flow needs before it may write.
pub(super) struct AccountMeta {
    pub locked: bool,
    pub eventually_consistent: bool,
    pub is_account_set: bool,
}

/// Everything the flow reads, in one statement.
pub(super) struct PostingState {
    pub epoch: i64,
    pub seeds: Vec<AccountMembership>,
    pub journals: HashMap<JournalId, JournalValues>,
    pub accounts: HashMap<AccountId, AccountMeta>,
    pub balances: HashMap<(JournalId, AccountId, Currency), BalanceSnapshot>,
    pub controls: HashMap<AccountId, (VelocityContextAccountValues, Vec<AccountVelocityControl>)>,
}

/// The ancestor phase's supplemental read (only when the flow resolved
/// non-EC ancestor sets).
pub(super) struct AncestorState {
    pub accounts: HashMap<AccountId, AccountMeta>,
    pub balances: HashMap<(JournalId, AccountId, Currency), BalanceSnapshot>,
    pub controls: HashMap<AccountId, (VelocityContextAccountValues, Vec<AccountVelocityControl>)>,
}

#[derive(Deserialize)]
struct SeedRow(AccountId, AccountSetId);

#[derive(Deserialize)]
struct AccountRow(AccountId, String, bool, bool);

#[derive(Deserialize)]
struct ControlRow(
    AccountId,
    AccountVelocityControl,
    VelocityContextAccountValues,
);

#[derive(Deserialize)]
struct JournalRow(JournalId, JournalValues);

/// Rows destined for one entity's table + events table.
#[derive(Default)]
struct EventRows {
    ids: Vec<uuid::Uuid>,
    sequences: Vec<i32>,
    event_types: Vec<String>,
    events: Vec<serde_json::Value>,
}

impl EventRows {
    fn push_initial(&mut self, id: uuid::Uuid, types: Vec<String>, events: Vec<serde_json::Value>) {
        self.ids.push(id);
        self.sequences.push(1);
        self.event_types
            .push(types.into_iter().next().expect("one initial event"));
        self.events
            .push(events.into_iter().next().expect("one initial event"));
    }
}

/// The transaction rows of one apply.
#[derive(Default)]
struct TransactionRows {
    ids: Vec<TransactionId>,
    journal_ids: Vec<JournalId>,
    template_ids: Vec<TxTemplateId>,
    external_ids: Vec<Option<String>>,
    correlation_ids: Vec<String>,
    effectives: Vec<NaiveDate>,
}

/// The entry rows of one apply.
#[derive(Default)]
struct EntryRows {
    ids: Vec<EntryId>,
    journal_ids: Vec<JournalId>,
    account_ids: Vec<AccountId>,
    transaction_ids: Vec<TransactionId>,
}

/// Every row [`PostingRepo::insert_postings_and_balances_in_op`] writes, accumulated per posting.
///
/// The push methods hydrate exactly as the entity repos would — serialize the
/// initial event, mark it persisted at `now`, rebuild the entity from its
/// events — and stage the table + event rows for the fused insert.
#[derive(Default)]
pub(super) struct PostingRows {
    transactions: TransactionRows,
    tx_events: EventRows,
    entries: EntryRows,
    entry_events: EventRows,
}

impl PostingRows {
    pub(super) fn push_transaction(
        &mut self,
        new_tx: NewTransaction,
        now: DateTime<Utc>,
    ) -> Transaction {
        use es_entity::{IntoEvents, TryFromEvents};
        let mut events = new_tx.into_events();
        let types = events.new_event_types();
        let serialized = events.serialize_new_events();
        events.mark_new_events_persisted_at(now);
        let transaction = Transaction::try_from_events(events).expect("transaction hydration");

        let values = transaction.values();
        self.transactions.ids.push(values.id);
        self.transactions.journal_ids.push(values.journal_id);
        self.transactions.template_ids.push(values.tx_template_id);
        self.transactions
            .external_ids
            .push(values.external_id.clone());
        self.transactions
            .correlation_ids
            .push(values.correlation_id.clone());
        self.transactions.effectives.push(values.effective);
        self.tx_events
            .push_initial(values.id.into(), types, serialized);
        transaction
    }

    pub(super) fn push_entry(&mut self, new_entry: NewEntry, now: DateTime<Utc>) -> Entry {
        use es_entity::{IntoEvents, TryFromEvents};
        let mut events = new_entry.into_events();
        let types = events.new_event_types();
        let serialized = events.serialize_new_events();
        events.mark_new_events_persisted_at(now);
        let entry = Entry::try_from_events(events).expect("entry hydration");

        let values = entry.values();
        self.entries.ids.push(values.id);
        self.entries.journal_ids.push(values.journal_id);
        self.entries.account_ids.push(values.account_id);
        self.entries.transaction_ids.push(values.transaction_id);
        self.entry_events
            .push_initial(values.id.into(), types, serialized);
        entry
    }
}

#[derive(Default)]
struct SnapshotColumns {
    journal_ids: Vec<JournalId>,
    account_ids: Vec<AccountId>,
    currencies: Vec<&'static str>,
    versions: Vec<i32>,
    entry_ids: Vec<EntryId>,
    values: Vec<serde_json::Value>,
}

impl From<&[BalanceSnapshot]> for SnapshotColumns {
    fn from(snapshots: &[BalanceSnapshot]) -> Self {
        let mut out = Self::default();
        for balance in snapshots {
            out.journal_ids.push(balance.journal_id);
            out.account_ids.push(balance.account_id);
            out.currencies.push(balance.currency.code());
            out.versions.push(balance.version as i32);
            out.entry_ids.push(balance.entry_id);
            out.values
                .push(serde_json::to_value(balance).expect("Failed to serialize balance snapshot"));
        }
        out
    }
}

/// The posting flow's repository: every statement the flow issues lives here,
/// and nowhere else — [`super::Postings`] (via [`super::TemplateCache`] for
/// templates) orchestrates, this struct talks to the database.
///
/// Stateless by design: every method is `_in_op`, executing on the caller's
/// atomic operation. It carries no pool because the flow never reads outside
/// an op.
#[derive(Clone)]
pub(super) struct PostingRepo;

impl PostingRepo {
    /// Phase 1: take the whole flow's locks, pin the transaction timestamp,
    /// and re-check the template versions preparation used — one statement.
    ///
    /// See the module doc for why the `ORDER BY` and `MATERIALIZED` are
    /// load-bearing. The template probe rides along because it reads nothing
    /// the fence protects: templates are independent of the balance rows being
    /// locked.
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.posting.lock_balances_and_probe_templates",
        skip_all,
        fields(pairs = keys.account_ids.len(), codes = codes.len()),
        err(level = "warn")
    )]
    pub(super) async fn lock_balances_and_probe_templates_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        keys: &BalanceKeys,
        codes: &[String],
        manual_now: Option<DateTime<Utc>>,
    ) -> Result<LockOutcome, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            WITH locks AS MATERIALIZED (
                SELECT
                    pg_advisory_xact_lock_shared($1::int4, hashtext(v.account_id::text)),
                    CASE WHEN NOT a.eventually_consistent THEN
                        pg_advisory_xact_lock(
                            hashtext(concat(v.journal_id::text, v.account_id::text, v.currency))
                        )
                    END
                FROM UNNEST($2::uuid[], $3::uuid[], $4::text[])
                    AS v(journal_id, account_id, currency)
                JOIN cala_accounts a ON a.id = v.account_id
                ORDER BY v.account_id, v.currency, v.journal_id
            ),
            templates AS (
                SELECT t.code, t.id, MAX(e.sequence)::int4 AS version
                FROM cala_tx_templates t
                JOIN cala_tx_template_events e ON t.id = e.id
                WHERE t.code = ANY($5::text[])
                GROUP BY t.code, t.id
            )
            SELECT
                COALESCE($6::timestamptz, NOW()) AS "now!",
                (SELECT COUNT(*) FROM locks) AS "locked!",
                t.code AS "code?",
                t.id AS "template_id?: TxTemplateId",
                t.version AS "version?"
            FROM (SELECT 1) AS anchor
            LEFT JOIN templates t ON TRUE
            "#,
            EC_SET_LOCK_CLASS,
            &keys.journal_ids as &[JournalId],
            &keys.account_ids as &[AccountId],
            &keys.currencies as &[&str],
            codes,
            manual_now,
        )
        .fetch_all(op.as_executor())
        .await?;

        let now = rows.first().expect("anchor row always present").now;
        let mut template_versions = HashMap::new();
        for row in rows {
            if let (Some(code), Some(id), Some(version)) = (row.code, row.template_id, row.version)
            {
                template_versions.insert(code, (id, version));
            }
        }
        Ok(LockOutcome {
            now,
            template_versions,
        })
    }

    /// Phase 2: every read the flow needs, in one statement.
    ///
    /// Runs strictly after [`Self::lock_balances_and_probe_templates_in_op`],
    /// so the membership probe and the balance rows are read under a snapshot
    /// taken *after* the locks were granted. The probe takes no locks of its own: the
    /// ancestors are unknowable until the seeds come back and are expanded,
    /// and locking guessed ancestors here would be unsound (an in-statement
    /// lock wait does not refresh the snapshot) as well as wasteful (a wrong
    /// guess forces a second corrective batch, breaking single-sorted-batch
    /// acquisition).
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.posting.read_posting_state",
        skip_all,
        fields(accounts = account_ids.len(), journals = journal_ids.len()),
        err(level = "warn")
    )]
    pub(super) async fn read_posting_state_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
        journal_ids: &[JournalId],
        keys: &BalanceKeys,
    ) -> Result<PostingState, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                (SELECT g.epoch FROM cala_account_set_graph_epoch g) AS "epoch!",
                (
                    SELECT jsonb_agg(jsonb_build_array(m.member_account_id, m.account_set_id))
                    FROM cala_account_set_member_accounts m
                    WHERE m.member_account_id = ANY($1::uuid[])
                ) AS "seeds",
                (
                    SELECT jsonb_agg(jsonb_build_array(
                        a.id, a.status::text, a.eventually_consistent, a.is_account_set
                    ))
                    FROM cala_accounts a
                    WHERE a.id = ANY($1::uuid[])
                ) AS "accounts",
                (
                    SELECT jsonb_agg(jsonb_build_array(
                        vc.account_id, vc.values, a.velocity_context_values
                    ))
                    FROM cala_velocity_account_controls vc
                    JOIN cala_accounts a ON a.id = vc.account_id
                    WHERE vc.account_id = ANY($1::uuid[])
                ) AS "controls",
                (
                    SELECT jsonb_agg(jsonb_build_array(j.id, j.values))
                    FROM (
                        SELECT DISTINCT ON (e.id) e.id, e.event -> 'values' AS values
                        FROM cala_journal_events e
                        WHERE e.id = ANY($2::uuid[])
                        ORDER BY e.id, e.sequence DESC
                    ) j
                ) AS "journals",
                (
                    SELECT jsonb_agg(b.latest_values)
                    FROM UNNEST($3::uuid[], $4::uuid[], $5::text[])
                        AS v(journal_id, account_id, currency)
                    JOIN cala_current_balances b
                      ON b.journal_id = v.journal_id
                     AND b.account_id = v.account_id
                     AND b.currency = v.currency
                ) AS "balances"
            "#,
            account_ids as &[AccountId],
            journal_ids as &[JournalId],
            &keys.journal_ids as &[JournalId],
            &keys.account_ids as &[AccountId],
            &keys.currencies as &[&str],
        )
        .fetch_one(op.as_executor())
        .await?;

        Ok(PostingState {
            epoch: row.epoch,
            seeds: Self::decode::<SeedRow>(row.seeds)
                .into_iter()
                .map(|SeedRow(account_id, account_set_id)| AccountMembership {
                    account_set_id,
                    account_id,
                })
                .collect(),
            journals: Self::decode::<JournalRow>(row.journals)
                .into_iter()
                .map(|JournalRow(id, values)| (id, values))
                .collect(),
            accounts: Self::index_accounts(Self::decode(row.accounts)),
            balances: Self::index_balances(Self::decode(row.balances)),
            controls: Self::index_controls(Self::decode(row.controls)),
        })
    }

    /// The ancestor phase's read: balances, status and velocity controls for
    /// the non-EC ancestor sets the in-memory expansion resolved.
    ///
    /// Runs strictly after `lock_resolved_ancestors_in_op`, preserving
    /// lock-before-read for the ancestor rows exactly as the entry pairs get
    /// it from [`Self::lock_balances_and_probe_templates_in_op`].
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.posting.read_ancestor_state",
        skip_all,
        fields(pairs = keys.account_ids.len()),
        err(level = "warn")
    )]
    pub(super) async fn read_ancestor_state_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        set_account_ids: &[AccountId],
        keys: &BalanceKeys,
    ) -> Result<AncestorState, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT
                (
                    SELECT jsonb_agg(jsonb_build_array(
                        a.id, a.status::text, a.eventually_consistent, a.is_account_set
                    ))
                    FROM cala_accounts a
                    WHERE a.id = ANY($1::uuid[])
                ) AS "accounts",
                (
                    SELECT jsonb_agg(jsonb_build_array(
                        vc.account_id, vc.values, a.velocity_context_values
                    ))
                    FROM cala_velocity_account_controls vc
                    JOIN cala_accounts a ON a.id = vc.account_id
                    WHERE vc.account_id = ANY($1::uuid[])
                ) AS "controls",
                (
                    SELECT jsonb_agg(b.latest_values)
                    FROM UNNEST($2::uuid[], $3::uuid[], $4::text[])
                        AS v(journal_id, account_id, currency)
                    JOIN cala_current_balances b
                      ON b.journal_id = v.journal_id
                     AND b.account_id = v.account_id
                     AND b.currency = v.currency
                ) AS "balances"
            "#,
            set_account_ids as &[AccountId],
            &keys.journal_ids as &[JournalId],
            &keys.account_ids as &[AccountId],
            &keys.currencies as &[&str],
        )
        .fetch_one(op.as_executor())
        .await?;

        Ok(AncestorState {
            accounts: Self::index_accounts(Self::decode(row.accounts)),
            balances: Self::index_balances(Self::decode(row.balances)),
            controls: Self::index_controls(Self::decode(row.controls)),
        })
    }

    /// Phase 3: every row the flow writes, in one statement.
    ///
    /// Transactions, entries, both event streams and the balance snapshots are
    /// independent inserts fused into a single multi-CTE statement. The two
    /// intra-statement foreign keys — `cala_transaction_events.id ->
    /// cala_transactions.id` and `cala_entry_events.id -> cala_entries.id`,
    /// plus `cala_balance_history -> cala_current_balances` — are satisfied
    /// because referential-integrity triggers are AFTER-ROW triggers fired at
    /// the end of the *statement*, once every CTE has run. (The balance pair
    /// already relied on exactly this before consolidation.)
    ///
    /// Balance snapshots are flushed in bounded sub-batches; each sub-batch is
    /// a self-contained statement, so a batch fanning into deep ancestor
    /// chains costs a few extra statements rather than one unbounded one.
    #[tracing::instrument(
        level = "debug",
        name = "cala_ledger.posting.insert_postings_and_balances",
        skip_all,
        fields(
            transactions = rows.transactions.ids.len(),
            entries = rows.entries.ids.len(),
            snapshots = snapshots.len()
        ),
        err(level = "warn")
    )]
    pub(super) async fn insert_postings_and_balances_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        now: DateTime<Utc>,
        rows: &PostingRows,
        snapshots: &[BalanceSnapshot],
    ) -> Result<(), sqlx::Error> {
        let (head, rest) = snapshots.split_at(snapshots.len().min(INSERT_SNAPSHOT_BATCH_SIZE));
        let balances = SnapshotColumns::from(head);

        sqlx::query!(
            r#"
            WITH tx AS (
                INSERT INTO cala_transactions
                    (id, journal_id, tx_template_id, external_id, correlation_id, effective, created_at)
                SELECT *, $7::timestamptz FROM UNNEST(
                    $1::uuid[], $2::uuid[], $3::uuid[], $4::text[], $5::text[], $6::date[]
                )
            ),
            tx_events AS (
                INSERT INTO cala_transaction_events (id, sequence, event_type, event, recorded_at)
                SELECT *, $7::timestamptz FROM UNNEST(
                    $8::uuid[], $9::int4[], $10::text[], $11::jsonb[]
                )
            ),
            entries AS (
                INSERT INTO cala_entries (id, journal_id, account_id, transaction_id, created_at)
                SELECT *, $7::timestamptz FROM UNNEST(
                    $12::uuid[], $13::uuid[], $14::uuid[], $15::uuid[]
                )
            ),
            entry_events AS (
                INSERT INTO cala_entry_events (id, sequence, event_type, event, recorded_at)
                SELECT *, $7::timestamptz FROM UNNEST(
                    $16::uuid[], $17::int4[], $18::text[], $19::jsonb[]
                )
            ),
            new_snapshots AS (
                INSERT INTO cala_balance_history
                    (journal_id, account_id, currency, version, latest_entry_id, values)
                SELECT * FROM UNNEST(
                    $20::uuid[], $21::uuid[], $22::text[], $23::int4[], $24::uuid[], $25::jsonb[]
                )
                RETURNING *
            )
            INSERT INTO cala_current_balances AS c
                (journal_id, account_id, currency, latest_version, latest_values)
            SELECT
                journal_id,
                account_id,
                currency,
                MAX(version) AS latest_version,
                (array_agg(values ORDER BY version DESC))[1] AS latest_values
            FROM new_snapshots
            GROUP BY journal_id, account_id, currency
            ON CONFLICT (account_id, journal_id, currency)
            DO UPDATE SET
                latest_version = GREATEST(c.latest_version, EXCLUDED.latest_version),
                latest_values = CASE
                    WHEN c.latest_version < EXCLUDED.latest_version
                    THEN EXCLUDED.latest_values
                    ELSE c.latest_values
                END
            "#,
            &rows.transactions.ids as &[TransactionId],
            &rows.transactions.journal_ids as &[JournalId],
            &rows.transactions.template_ids as &[TxTemplateId],
            &rows.transactions.external_ids as &[Option<String>],
            &rows.transactions.correlation_ids,
            &rows.transactions.effectives,
            now,
            &rows.tx_events.ids,
            &rows.tx_events.sequences,
            &rows.tx_events.event_types,
            &rows.tx_events.events,
            &rows.entries.ids as &[EntryId],
            &rows.entries.journal_ids as &[JournalId],
            &rows.entries.account_ids as &[AccountId],
            &rows.entries.transaction_ids as &[TransactionId],
            &rows.entry_events.ids,
            &rows.entry_events.sequences,
            &rows.entry_events.event_types,
            &rows.entry_events.events,
            &balances.journal_ids as &[JournalId],
            &balances.account_ids as &[AccountId],
            &balances.currencies as &[&str],
            &balances.versions,
            &balances.entry_ids as &[EntryId],
            &balances.values,
        )
        .execute(op.as_executor())
        .await?;

        for chunk in rest.chunks(INSERT_SNAPSHOT_BATCH_SIZE) {
            self.insert_snapshots_in_op(op, chunk).await?;
        }
        Ok(())
    }

    /// Cold-path template resolution: the `(id, latest version)` and the body
    /// for codes this process has not posted before, or whose cached version
    /// the fence statement reported stale. Called only by
    /// [`super::TemplateCache`]; two round trips, paid once per code per
    /// process.
    pub(super) async fn resolve_templates_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        codes: &[String],
    ) -> Result<HashMap<String, (TxTemplateId, i32, serde_json::Value)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            WITH latest AS (
                SELECT t.code, t.id, MAX(e.sequence)::int4 AS version
                FROM cala_tx_templates t
                JOIN cala_tx_template_events e ON t.id = e.id
                WHERE t.code = ANY($1::text[])
                GROUP BY t.code, t.id
            )
            SELECT
                l.code AS "code!",
                l.id AS "id!: TxTemplateId",
                l.version AS "version!",
                e.event AS "event!"
            FROM latest l
            JOIN cala_tx_template_events e
              ON e.id = l.id AND e.sequence = l.version
            "#,
            codes,
        )
        .fetch_all(op.as_executor())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| (row.code, (row.id, row.version, row.event)))
            .collect())
    }

    /// Overflow flush for [`Self::insert_postings_and_balances_in_op`]'s snapshot sub-batching.
    async fn insert_snapshots_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        snapshots: &[BalanceSnapshot],
    ) -> Result<(), sqlx::Error> {
        let balances = SnapshotColumns::from(snapshots);
        sqlx::query!(
            r#"
            WITH new_snapshots AS (
                INSERT INTO cala_balance_history
                    (journal_id, account_id, currency, version, latest_entry_id, values)
                SELECT * FROM UNNEST(
                    $1::uuid[], $2::uuid[], $3::text[], $4::int4[], $5::uuid[], $6::jsonb[]
                )
                RETURNING *
            )
            INSERT INTO cala_current_balances AS c
                (journal_id, account_id, currency, latest_version, latest_values)
            SELECT
                journal_id,
                account_id,
                currency,
                MAX(version) AS latest_version,
                (array_agg(values ORDER BY version DESC))[1] AS latest_values
            FROM new_snapshots
            GROUP BY journal_id, account_id, currency
            ON CONFLICT (account_id, journal_id, currency)
            DO UPDATE SET
                latest_version = GREATEST(c.latest_version, EXCLUDED.latest_version),
                latest_values = CASE
                    WHEN c.latest_version < EXCLUDED.latest_version
                    THEN EXCLUDED.latest_values
                    ELSE c.latest_values
                END
            "#,
            &balances.journal_ids as &[JournalId],
            &balances.account_ids as &[AccountId],
            &balances.currencies as &[&str],
            &balances.versions,
            &balances.entry_ids as &[EntryId],
            &balances.values,
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    fn decode<T: serde::de::DeserializeOwned>(value: Option<serde_json::Value>) -> Vec<T> {
        match value {
            Some(serde_json::Value::Null) | None => Vec::new(),
            Some(value) => {
                serde_json::from_value(value).expect("posting read: malformed aggregate")
            }
        }
    }

    fn index_accounts(rows: Vec<AccountRow>) -> HashMap<AccountId, AccountMeta> {
        rows.into_iter()
            .map(
                |AccountRow(id, status, eventually_consistent, is_account_set)| {
                    (
                        id,
                        AccountMeta {
                            locked: status == "locked",
                            eventually_consistent,
                            is_account_set,
                        },
                    )
                },
            )
            .collect()
    }

    fn index_balances(
        snapshots: Vec<BalanceSnapshot>,
    ) -> HashMap<(JournalId, AccountId, Currency), BalanceSnapshot> {
        snapshots
            .into_iter()
            .map(|s| ((s.journal_id, s.account_id, s.currency), s))
            .collect()
    }

    #[allow(clippy::type_complexity)]
    fn index_controls(
        rows: Vec<ControlRow>,
    ) -> HashMap<AccountId, (VelocityContextAccountValues, Vec<AccountVelocityControl>)> {
        let mut out: HashMap<AccountId, (VelocityContextAccountValues, Vec<_>)> = HashMap::new();
        for ControlRow(account_id, control, context) in rows {
            out.entry(account_id)
                .or_insert_with(|| (context, Vec::new()))
                .1
                .push(control);
        }
        out
    }
}
