use es_entity::*;
use sqlx::PgPool;
use tracing::instrument;

use std::collections::{HashMap, HashSet};

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, JournalId},
};

use super::{
    entity::*,
    error::*,
    graph_validation::{AccountMembership, SetMembership},
};

/// Coarse advisory lock guarding the account-set membership graph
/// (`cala_account_set_member_accounts` and
/// `cala_account_set_member_account_sets`).
///
/// Membership is stored as **direct edges only** — ancestor sets are
/// resolved at read time by the epoch-validated set-graph cache
/// (`SetGraphCache` in `account_set/graph_cache.rs`: in-memory
/// expansion over a cached edge snapshot, with the op-local
/// recursive-walk fallback `walk_mappings_and_lock_in_op`); there is
/// no materialized transitive closure. Structure mutations bump
/// `cala_account_set_graph_epoch` under the exclusive coarse lock so
/// every cached snapshot is validated per resolution. The graph still
/// carries a
/// load-bearing invariant that every mutation must validate before
/// writing: **path uniqueness** — an account may be contained in any
/// given set via at most one membership path (double membership is
/// prohibited). The old closure enforced this incidentally through its
/// unique constraint; walk-only enforces it explicitly
/// (`assert_no_double_membership` and the combined-graph set-structure
/// validation, `SetGraphCache::assert_valid_set_memberships_in_op`).
/// Each check is a read-then-write over the graph,
/// so it must be fenced against concurrent writers — which is why the
/// closure-era lock protocol survives walk-only unchanged:
///
/// - Set-structure mutations (single and batched set attachment —
///   `AccountSets::add_member_in_op`'s set arm and
///   `add_member_sets_in_op`, both via [`Self::lock_for_set_membership_op`]
///   — and `remove_member_set`) take this lock EXCLUSIVE. They mutate
///   the edges that every walk
///   reads, and read the member rows that account-member mutations
///   write, so they must exclude everything.
/// - Account-member mutations (`AccountSets::add_member(s)_in_op` /
///   `remove_member_in_op`'s account arms) take this lock SHARED
///   ([`Self::lock_graph_shared_in_op`]) plus an EXCLUSIVE per-member
///   lock (`MEMBER_LOCK_CLASS`, `crate::account_set_member` — the
///   module that owns the member-edge table's writes and keyed on the
///   member account id). The two locks are two separate awaited
///   statements — coarse in `account_set/repo.rs`, per-member in
///   `account_set_member` — sequenced by the `AccountSets` SERVICE
///   (`mod.rs`), which is what guarantees the acquisition order.
///   Shared-vs-exclusive fences them against structure mutations,
///   while account-member mutations for *different* members run
///   concurrently — each validation involves only its own member's
///   paths. The per-member lock serializes mutations touching the
///   *same* member, whose interleaved check-then-write sequences could
///   otherwise commit a double membership.
/// - The create-inside-set fast path (`Accounts::create_*` with
///   `NewAccount::initial_account_set`) takes NEITHER this lock nor
///   any class-1 lock — only the class-2 per-member EXCLUSIVE, via
///   `crate::account_set_member`. Sound only because the account is
///   created in the same op with exactly one membership; the invariant
///   argument lives on the `NewAccount::initial_account_set` field
///   docs.
///
/// Ordering: the coarse lock is always acquired before the per-member
/// lock. An operation must never wait on the coarse lock while holding
/// a per-member lock — under PostgreSQL's FIFO lock queueing that can
/// form a wait cycle with a queued exclusive (structure) waiter. (The
/// fast path never touches the coarse lock at all, so it cannot
/// violate this.)
///
/// Key-space hygiene: the lock lives in the 2-arg advisory key space
/// under its own `classid` ([`GRAPH_LOCK_CLASS`]). The 1-arg space is
/// shared with the `hashtext(journal‖account‖currency)` per-balance
/// poster locks and the velocity balance locks, so a tuple hashing to
/// exactly 123456 there would take the graph lock EXCLUSIVE from the
/// poster path (silent global serialization, ~2⁻³² per key). The 1-arg
/// and 2-arg forms are DIFFERENT lock spaces that do not mutually
/// exclude, so every acquisition site must always agree on the form —
/// they all go through this module and use ([`GRAPH_LOCK_CLASS`],
/// [`ADDVISORY_LOCK_ID`]).
const ADDVISORY_LOCK_ID: i32 = 123456;

/// `classid` namespace for the coarse membership-graph lock (2-arg
/// form), keyed on [`ADDVISORY_LOCK_ID`]. Must stay disjoint from
/// `EC_SET_LOCK_CLASS` (= 1) and `MEMBER_LOCK_CLASS` (= 2, in
/// `crate::account_set_member`).
const GRAPH_LOCK_CLASS: i32 = 3;

use account_set_cursor::*;

#[derive(EsRepo, Debug, Clone)]
#[es_repo(
    entity = "AccountSet",
    columns(
        name(
            ty = "String",
            update(accessor = "values().name"),
            list_by,
            list_for(by(created_at))
        ),
        journal_id(ty = "JournalId", update(persist = false)),
        external_id(
            ty = "Option<String>",
            update(accessor = "values().external_id"),
            list_by
        ),
    ),
    tbl_prefix = "cala",
    post_persist_hook = "publish",
    persist_event_context = false
)]
pub(super) struct AccountSetRepo {
    pool: PgPool,
    publisher: OutboxPublisher,
}

impl AccountSetRepo {
    pub fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            pool: pool.clone(),
            publisher: publisher.clone(),
        }
    }

    /// Take the SHARED half of the coarse membership-graph lock protocol
    /// for an account-member mutation (see [`ADDVISORY_LOCK_ID`]). The
    /// EXCLUSIVE per-member lock is taken separately, by
    /// `AccountSetMembers::lock_members_in_op`
    /// (`crate::account_set_member`) — the `AccountSets` service
    /// sequences the two so acquisition order (coarse before per-member)
    /// is guaranteed.
    pub(super) async fn lock_graph_shared_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            "SELECT pg_advisory_xact_lock_shared($1, $2)",
            GRAPH_LOCK_CLASS,
            ADDVISORY_LOCK_ID
        )
        .execute(db.as_executor())
        .await?;
        Ok(())
    }

    /// Rejects account-member additions that would give an account a second
    /// membership path to any set (double membership — see
    /// [`ADDVISORY_LOCK_ID`]). Containment paths are counted in ONE
    /// recursive walk seeded by both the new `(account_set_id, account_id)`
    /// pairs and the accounts' existing direct memberships: a recursive
    /// UNION ALL walk from the union of seeds equals the union of the
    /// per-seed walks, path multiplicity included, so any `(account, set)`
    /// the same account reaches twice — via an existing path, via the new
    /// edge, or across two pairs of the same batch — surfaces as a group
    /// with more than one row. Reported as
    /// [`AccountSetError::MemberAlreadyAdded`], the same error the old
    /// closure's unique-constraint collision produced.
    ///
    /// The single-walk form is also what keeps the plan sane once the
    /// prepared statement flips to a generic plan (PostgreSQL switches
    /// after five executions): with a merged seed set the planner hashes
    /// the invariant edge table ONCE per call and probes it with the
    /// recursion worktable. Split into two walks (the previous form), the
    /// worktable estimate is small enough that the planner hashes the
    /// worktable instead and re-scans the whole edge table at every
    /// recursion level — measured at ~5 full seq scans of
    /// `cala_account_set_member_account_sets` per call under load, the
    /// dominant DB cost of the attach path.
    ///
    /// Must run under the account-member lock protocol: the walk is a read
    /// over the set graph and the account's direct edges, and the insert
    /// that follows relies on its result staying valid until commit.
    ///
    /// This is the check's rare path: the set-graph cache
    /// (`SetGraphCache::assert_no_double_membership_in_op`) resolves the
    /// common case with an in-memory path count over its epoch-validated
    /// edge snapshot and only falls back here on an epoch mismatch or an
    /// unknown set id.
    pub(super) async fn assert_no_double_membership(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        members: &[AccountMembership],
    ) -> Result<(), AccountSetError> {
        // The parallel arrays the UNNEST needs are built here, at the SQL
        // boundary, so the ordered-pair shape never escapes into the caller.
        let account_set_ids: Vec<AccountSetId> = members.iter().map(|m| m.account_set_id).collect();
        let account_ids: Vec<AccountId> = members.iter().map(|m| m.account_id).collect();
        let row = sqlx::query!(
            r#"
            WITH RECURSIVE all_seeds AS (
                SELECT v.account_id, v.account_set_id
                FROM UNNEST($1::uuid[], $2::uuid[]) AS v(account_set_id, account_id)

                UNION ALL
                SELECT m.member_account_id AS account_id, m.account_set_id
                FROM cala_account_set_member_accounts m
                WHERE m.member_account_id = ANY($2)
            ),
            containments AS (
                SELECT account_id, account_set_id FROM all_seeds

                UNION ALL
                SELECT c.account_id, e.account_set_id
                FROM containments c
                JOIN cala_account_set_member_account_sets e
                    ON e.member_account_set_id = c.account_set_id
            )
            SELECT EXISTS (
                SELECT 1 FROM containments
                GROUP BY account_id, account_set_id
                HAVING COUNT(*) > 1
            ) AS "conflict!"
            "#,
            &account_set_ids as &[AccountSetId],
            &account_ids as &[AccountId],
        )
        .fetch_one(db.as_executor())
        .await?;
        if row.conflict {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        Ok(())
    }

    /// Take the exclusive membership-graph lock for a structure mutation.
    /// Held until `db` commits or rolls back.
    pub(super) async fn lock_for_set_membership_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
    ) -> Result<(), AccountSetError> {
        sqlx::query!(
            "SELECT pg_advisory_xact_lock($1, $2)",
            GRAPH_LOCK_CLASS,
            ADDVISORY_LOCK_ID
        )
        .execute(db.as_executor())
        .await?;
        Ok(())
    }

    /// Read the membership-graph epoch in this operation's snapshot.
    pub(super) async fn fetch_set_graph_epoch_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
    ) -> Result<i64, AccountSetError> {
        Ok(
            sqlx::query_scalar("SELECT epoch FROM cala_account_set_graph_epoch")
                .fetch_one(db.as_executor())
                .await?,
        )
    }

    /// Load all committed set-to-set edges as an op-local cache fallback.
    ///
    /// The graph is intentionally read with one flat query. It is small, and
    /// this path runs only when the epoch-validated snapshot is cold or stale;
    /// avoiding a recursive component walk keeps fallback work bounded by the
    /// edge table rather than graph depth.
    pub(super) async fn fetch_set_membership_edges_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
    ) -> Result<Vec<SetMembership>, AccountSetError> {
        let rows: Vec<(AccountSetId, AccountSetId)> = sqlx::query_as(
            r#"
          SELECT account_set_id, member_account_set_id
          FROM cala_account_set_member_account_sets
          "#,
        )
        .fetch_all(db.as_executor())
        .await?;
        Ok(rows.into_iter().map(SetMembership::from).collect())
    }

    /// Load only direct account memberships that can participate in a conflict
    /// introduced by `members` against the supplied existing edge graph.
    pub(super) async fn fetch_affected_account_memberships_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        existing_edges: &[SetMembership],
        members: &[SetMembership],
    ) -> Result<Vec<AccountMembership>, AccountSetError> {
        let member_account_set_ids: Vec<AccountSetId> = members
            .iter()
            .map(|edge| edge.member_account_set_id)
            .collect();

        // Only accounts below a proposed member endpoint can gain a new
        // containment path. Compute that affected descendant closure over the
        // final graph so interactions between proposed edges are included.
        let mut children: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        for edge in existing_edges.iter().chain(members) {
            children
                .entry(edge.account_set_id)
                .or_default()
                .push(edge.member_account_set_id);
        }
        let mut affected_set_ids = HashSet::new();
        let mut pending = member_account_set_ids;
        while let Some(account_set_id) = pending.pop() {
            if affected_set_ids.insert(account_set_id) {
                pending.extend(children.get(&account_set_id).into_iter().flatten().copied());
            }
        }
        let affected_set_ids: Vec<_> = affected_set_ids.into_iter().collect();

        // The committed graph is already account-path valid. Therefore every
        // conflict introduced by this batch includes an account attached in
        // the affected descendant closure above. Find those candidate accounts
        // through the set-leading index, then load all of their direct
        // memberships through the member-leading unique index. This preserves
        // complete account-path validation without reading every membership in
        // the connected component.
        //
        // These queries decode into newtype tuples rather than using sqlx's
        // compile-time checked `query!`, because the column types are stable
        // and the tuple shape is local to this module. An `ORDER BY` keeps
        // the candidate and membership rows deterministic so the same conflict
        // is reported consistently across runs.
        let candidate_account_ids: Vec<AccountId> = sqlx::query_scalar(
            r#"
          SELECT DISTINCT member_account_id
          FROM cala_account_set_member_accounts
          WHERE account_set_id = ANY($1)
          ORDER BY member_account_id
          "#,
        )
        .bind(&affected_set_ids)
        .fetch_all(db.as_executor())
        .await?;
        if candidate_account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(AccountSetId, AccountId)> = sqlx::query_as(
            r#"
          SELECT account_set_id, member_account_id
          FROM cala_account_set_member_accounts
          WHERE member_account_id = ANY($1)
          ORDER BY member_account_id, account_set_id
          "#,
        )
        .bind(&candidate_account_ids)
        .fetch_all(db.as_executor())
        .await?;
        Ok(rows.into_iter().map(AccountMembership::from).collect())
    }

    /// Insert a validated batch of direct account-set membership edges.
    ///
    /// Precondition: the caller holds [`Self::lock_for_set_membership_op`]
    /// and has passed the graph cache's combined-graph validation in this
    /// same op. This function only persists: it inserts the edges, bumps the
    /// graph epoch once for the whole batch, and publishes one outbox event
    /// per edge. All cycle, path, account, and depth checks happen in the
    /// caller's validation step before this runs.
    #[instrument(
        level = "debug",
        name = "account_set.insert_member_sets",
        skip_all,
        fields(count = members.len()),
        err(level = "warn")
    )]
    pub(super) async fn insert_member_sets(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        members: &[SetMembership],
    ) -> Result<(), AccountSetError> {
        if members.is_empty() {
            return Ok(());
        }

        let account_set_ids: Vec<AccountSetId> =
            members.iter().map(|edge| edge.account_set_id).collect();
        let member_account_set_ids: Vec<AccountSetId> = members
            .iter()
            .map(|edge| edge.member_account_set_id)
            .collect();

        sqlx::query(
            r#"
          INSERT INTO cala_account_set_member_account_sets
            (account_set_id, member_account_set_id)
          SELECT account_set_id, member_account_set_id
          FROM UNNEST($1::uuid[], $2::uuid[])
            AS proposed(account_set_id, member_account_set_id)
          "#,
        )
        .bind(&account_set_ids)
        .bind(&member_account_set_ids)
        .execute(db.as_executor())
        .await?;

        sqlx::query!("UPDATE cala_account_set_graph_epoch SET epoch = epoch + 1")
            .execute(db.as_executor())
            .await?;

        self.publisher
            .publish_all(
                db,
                members.iter().map(|edge| {
                    crate::outbox::OutboxEventPayload::AccountSetMemberCreated {
                        account_set_id: edge.account_set_id,
                        member_id: crate::account_set::AccountSetMemberId::AccountSet(
                            edge.member_account_set_id,
                        ),
                    }
                }),
            )
            .await?;

        Ok(())
    }

    #[instrument(
        level = "debug",
        name = "account_set.remove_member_set",
        skip_all,
        err(level = "warn")
    )]
    pub async fn remove_member_set(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member_account_set_id: AccountSetId,
    ) -> Result<(), AccountSetError> {
        // Structure mutation: EXCLUSIVE coarse lock (see ADDVISORY_LOCK_ID).
        sqlx::query!(
            "SELECT pg_advisory_xact_lock($1, $2)",
            GRAPH_LOCK_CLASS,
            ADDVISORY_LOCK_ID
        )
        .execute(db.as_executor())
        .await?;
        // Delete the single direct set->set edge. There are no
        // materialized ancestor/member rows to scrub.
        sqlx::query!(
            r#"
          DELETE FROM cala_account_set_member_account_sets
          WHERE account_set_id = $1 AND member_account_set_id = $2
          "#,
            account_set_id as AccountSetId,
            member_account_set_id as AccountSetId,
        )
        .execute(db.as_executor())
        .await?;

        // Invalidate every in-process set-graph cache snapshot (see
        // account_set/graph_cache.rs). Serialized with the edge delete
        // under the exclusive coarse lock held above.
        sqlx::query!("UPDATE cala_account_set_graph_epoch SET epoch = epoch + 1")
            .execute(db.as_executor())
            .await?;

        self.publisher
            .publish_all(
                db,
                std::iter::once(crate::outbox::OutboxEventPayload::AccountSetMemberRemoved {
                    account_set_id,
                    member_id: crate::account_set::AccountSetMemberId::AccountSet(
                        member_account_set_id,
                    ),
                }),
            )
            .await?;

        Ok(())
    }

    pub async fn find_where_account_is_member(
        &self,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        self.find_where_account_is_member_in_op(&self.pool, account_id, query)
            .await
    }

    pub async fn find_where_account_is_member_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_id: AccountId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        let (entities, has_next_page) = es_entity::es_query!(
            tbl_prefix = "cala",
            r#"SELECT a.id, a.name, a.created_at
              FROM cala_account_sets a
              JOIN cala_account_set_member_accounts asm
              ON asm.account_set_id = a.id
              WHERE asm.member_account_id = $1
              AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
              ORDER BY a.name, a.id
              LIMIT $4"#,
            account_id as AccountId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_n(op, query.first)
        .await?;

        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    pub async fn find_where_account_set_is_member(
        &self,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        self.find_where_account_set_is_member_in_op(&self.pool, account_set_id, query)
            .await
    }

    pub async fn find_where_account_set_is_member_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        account_set_id: AccountSetId,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
    {
        let (entities, has_next_page) = es_entity::es_query!(
            tbl_prefix = "cala",
            r#"SELECT a.id, a.name, a.created_at
               FROM cala_account_sets a
               JOIN cala_account_set_member_account_sets asm
               ON asm.account_set_id = a.id
               WHERE asm.member_account_set_id = $1
               AND ((a.name, a.id) > ($3, $2) OR ($3 IS NULL AND $2 IS NULL))
               ORDER BY a.name, a.id
               LIMIT $4"#,
            account_set_id as AccountSetId,
            query.after.as_ref().map(|c| c.id) as Option<AccountSetId>,
            query.after.map(|c| c.name),
            query.first as i64 + 1
        )
        .fetch_n(op, query.first)
        .await?;
        let mut end_cursor = None;
        if let Some(last) = entities.last() {
            end_cursor = Some(AccountSetByNameCursor {
                id: last.values().id,
                name: last.values().name.clone(),
            });
        }
        Ok(es_entity::PaginatedQueryRet {
            entities,
            has_next_page,
            end_cursor,
        })
    }

    /// One statement, one snapshot: the given accounts' **direct** set
    /// memberships plus the current set-graph epoch. This is the
    /// set-graph cache's hot-path read (posting-path ancestor resolution
    /// AND the double-membership check) — the epoch rides in the same
    /// statement, so an epoch match proves the cached edge graph equals
    /// the committed graph at this statement's snapshot.
    ///
    /// Anchored on the always-present epoch row: the epoch comes back
    /// even when the accounts have no direct memberships at all
    /// (`seeds` empty). The membership check needs exactly that case —
    /// zero existing memberships is its dominant input, and it still has
    /// to validate the new pairs against the epoch-matched cached graph.
    ///
    /// Deliberately takes NO locks: the ancestors are unknowable until
    /// the seeds come back and are expanded. Locking "assumed" ancestors
    /// here optimistically would be unsound twice over — an advisory
    /// lock wait inside a statement does not refresh that statement's
    /// snapshot (taken at statement start), the stale-read class the
    /// attach fence closes; and a wrong guess (guaranteed for a freshly
    /// created account, the dominant posting pattern) would force a
    /// second corrective lock batch, breaking the single-Rust-sorted-batch
    /// acquisition that poster-vs-poster deadlock-freedom rests on.
    pub(super) async fn probe_direct_memberships_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<DirectMembershipProbe, AccountSetError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                g.epoch AS "epoch!",
                m.member_account_id AS "account_id?: AccountId",
                m.account_set_id AS "set_id?: AccountSetId"
            FROM cala_account_set_graph_epoch g
            LEFT JOIN cala_account_set_member_accounts m
                ON m.member_account_id = ANY($1)
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        // The epoch table's one row is created by the migration; an empty
        // result cannot legitimately happen. Degrade to a value below
        // every possible snapshot epoch (DB epochs are >= 0, the cache's
        // cold sentinel is -1) rather than panic, so every consumer takes
        // its correct-by-construction fallback on this impossible input.
        let epoch = rows.first().map(|row| row.epoch).unwrap_or(i64::MIN);
        Ok(DirectMembershipProbe {
            epoch,
            seeds: rows
                .into_iter()
                .filter_map(|row| {
                    Some(AccountMembership {
                        account_set_id: row.set_id?,
                        account_id: row.account_id?,
                    })
                })
                .collect(),
        })
    }

    /// The set-graph cache's rare-path fallback (cold cache, epoch
    /// mismatch, unknown set id): resolve each entry account's ancestor
    /// sets AND take the poster's per-balance FOR_UPDATE locks on the
    /// non-EC ancestors' balance rows, in the same statement and round
    /// trip.
    ///
    /// Input is the posting's distinct `(account_id, currency)` entry
    /// pairs (parallel arrays). Each leaf's currencies propagate to
    /// exactly *its own* ancestors — the locked rows are precisely the
    /// `(ancestor, currency)` combinations the inline fan-out will
    /// write, no more (an ancestor reached only by a USD entry is not
    /// locked for another entry's BTC).
    ///
    /// The ancestors are unknowable before this walk runs, so their
    /// per-balance locks cannot join the poster's pre-insert lock
    /// prelude (`BalanceRepo::lock_entry_balances_in_op`, which covers
    /// the entry accounts). Taking them here is sound because this
    /// statement reads only the membership graph — never balance
    /// values; the balance read happens in a *later* statement
    /// (`find_for_update`'s data fetch), so lock-before-read holds
    /// across statements. The `locks` CTE is forced to execute by the
    /// scalar subquery in the outer WHERE; its ORDER BY makes
    /// acquisition order canonical (volatile lock calls are postponed
    /// until after the Sort — see the ordering doctrine on
    /// `BalanceRepo::lock_entry_balances_in_op`). Entry accounts and
    /// ancestor sets are disjoint key classes (the set-guard FK), and every
    /// poster acquires the two phases in the same order, so the split
    /// acquisition cannot deadlock posters against each other.
    #[instrument(
        level = "debug",
        name = "account_set.walk_mappings_and_lock_in_op",
        skip_all,
        err(level = "warn")
    )]
    pub(super) async fn walk_mappings_and_lock_in_op(
        &self,
        op: impl es_entity::IntoOneTimeExecutor<'_>,
        journal_id: JournalId,
        (account_ids, currencies): &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        // Adjacency-only membership: resolve each account's ancestor sets
        // by an upward recursive walk over the (tiny) set->set edge table,
        // seeded from the account's direct set memberships. UNION (not
        // UNION ALL) dedups and keeps the walk terminating even if a stray
        // edge slipped past the write-side cycle check.
        let rows = op.into_executor().fetch_all(sqlx::query!(
            r#"
          WITH RECURSIVE seed AS (
              SELECT DISTINCT m.member_account_id AS account_id, m.account_set_id
              FROM cala_account_set_member_accounts m
              WHERE m.member_account_id = ANY($2)
          ),
          ancestors AS (
              SELECT account_id, account_set_id FROM seed
              UNION
              SELECT a.account_id, e.account_set_id
              FROM ancestors a
              JOIN cala_account_set_member_account_sets e
                ON e.member_account_set_id = a.account_set_id
          ),
          resolved AS (
              SELECT a.account_id, a.account_set_id
              FROM ancestors a
              JOIN cala_account_sets s
                ON s.id = a.account_set_id AND s.journal_id = $1
          ),
          locks AS (
              SELECT pg_advisory_xact_lock(
                  hashtext(concat($1::text, t.account_set_id::text, t.currency))
              )
              FROM (
                  SELECT DISTINCT r.account_set_id, v.currency
                  FROM resolved r
                  JOIN UNNEST($2::uuid[], $3::text[]) AS v(account_id, currency)
                    ON v.account_id = r.account_id
                  JOIN cala_accounts acc
                    ON acc.id = r.account_set_id
                   AND NOT acc.eventually_consistent
              ) t
              ORDER BY t.account_set_id, t.currency
          )
          SELECT DISTINCT r.account_id AS "account_id!: AccountId", r.account_set_id AS "set_id!: AccountSetId"
          FROM resolved r
          WHERE (SELECT COUNT(*) FROM locks) IS NOT NULL
          "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
            currencies as &[&str],
        ))
        .await?;
        let mut mappings = HashMap::new();
        for row in rows {
            mappings
                .entry(row.account_id)
                .or_insert_with(Vec::new)
                .push(row.set_id);
        }
        Ok(mappings)
    }

    /// The memory path's lock statement: take the poster's per-balance
    /// FOR_UPDATE locks on the non-EC ancestor `(set, currency)` pairs
    /// the in-memory expansion resolved — the same keys the fallback's
    /// `locks` CTE takes, in the same 1-arg advisory namespace as
    /// `BalanceRepo::lock_entry_balances_in_op`'s entry pairs.
    ///
    /// Invoked immediately after expansion and strictly BEFORE
    /// `find_for_update`'s balance data fetch — sound for the same
    /// reason as the fallback's in-walk locks: expansion read only the
    /// membership graph, never balance values, so lock-before-read
    /// holds across statements.
    ///
    /// Lock-ordering invariant: the caller passes the pairs **deduped
    /// and Rust-sorted** (`(set_id, currency)` — uuid byte order
    /// matches Postgres uuid comparison, and currency codes are ASCII,
    /// so this is the same canonical order as the fallback CTE's
    /// `ORDER BY`). The join-free UNNEST scan evaluates the volatile
    /// lock calls row by row in array order, so no in-query Sort is
    /// needed (the join-free form — no join, no planner
    /// reordering to defend against). Every poster takes exactly ONE
    /// sorted ancestor lock batch per posting — here or in the
    /// fallback's CTE, never both — which is what keeps acquisition
    /// order canonical across posters.
    pub(super) async fn lock_resolved_ancestors_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        (set_ids, currencies): &(Vec<AccountSetId>, Vec<&str>),
    ) -> Result<(), AccountSetError> {
        if set_ids.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtext(concat($1::text, v.set_id::text, v.currency))
            )
            FROM UNNEST($2::uuid[], $3::text[]) AS v(set_id, currency)
            "#,
            journal_id as JournalId,
            set_ids as &[AccountSetId],
            currencies as &[&str],
        )
        .execute(op.as_executor())
        .await?;
        Ok(())
    }

    /// Meta + upward edges for specific sets, on the op executor (sees
    /// the op's own uncommitted set creations). The set-graph cache's
    /// op-local supplement for seed ids unknown to its shared snapshot.
    pub(super) async fn fetch_set_graph_nodes_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        set_ids: &[AccountSetId],
    ) -> Result<Vec<SetGraphNode>, AccountSetError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                s.id AS "set_id!: AccountSetId",
                s.journal_id AS "journal_id!: JournalId",
                acc.eventually_consistent AS "eventually_consistent!",
                e.account_set_id AS "parent_id?: AccountSetId"
            FROM cala_account_sets s
            JOIN cala_accounts acc
              ON acc.id = s.id
            LEFT JOIN cala_account_set_member_account_sets e
              ON e.member_account_set_id = s.id
            WHERE s.id = ANY($1)
            "#,
            set_ids as &[AccountSetId],
        )
        .fetch_all(op.as_executor())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SetGraphNode {
                id: row.set_id,
                journal_id: row.journal_id,
                eventually_consistent: row.eventually_consistent,
                parent_id: row.parent_id,
            })
            .collect())
    }

    /// The whole set graph (every set's meta + upward edges) plus the
    /// epoch, from the **pool** — committed data only, in one statement
    /// so epoch and graph come from a single snapshot. The set-graph
    /// cache's refresh read. Anchoring on the always-present epoch row
    /// guarantees >=1 row even with zero account sets.
    pub(super) async fn fetch_set_graph(&self) -> Result<SetGraphData, AccountSetError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                g.epoch AS "epoch!",
                s.id AS "set_id?: AccountSetId",
                s.journal_id AS "journal_id?: JournalId",
                acc.eventually_consistent AS "eventually_consistent?",
                e.account_set_id AS "parent_id?: AccountSetId"
            FROM cala_account_set_graph_epoch g
            LEFT JOIN cala_account_sets s ON TRUE
            LEFT JOIN cala_accounts acc ON acc.id = s.id
            LEFT JOIN cala_account_set_member_account_sets e
              ON e.member_account_set_id = s.id
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let epoch = rows.first().map(|row| row.epoch).unwrap_or_default();
        let nodes = rows
            .into_iter()
            .filter_map(|row| {
                let (Some(id), Some(journal_id), Some(eventually_consistent)) =
                    (row.set_id, row.journal_id, row.eventually_consistent)
                else {
                    return None;
                };
                Some(SetGraphNode {
                    id,
                    journal_id,
                    eventually_consistent,
                    parent_id: row.parent_id,
                })
            })
            .collect();
        Ok(SetGraphData { epoch, nodes })
    }

    async fn publish(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        entity: &AccountSet,
        new_events: es_entity::LastPersisted<'_, AccountSetEvent>,
    ) -> Result<(), sqlx::Error> {
        self.publisher
            .publish_entity_events(op, entity, new_events)
            .await?;
        Ok(())
    }
}

/// Result of [`AccountSetRepo::probe_direct_memberships_in_op`]: the
/// live `(account, direct set)` seed pairs and the set-graph epoch, read
/// in one snapshot.
pub(super) struct DirectMembershipProbe {
    pub epoch: i64,
    pub seeds: Vec<AccountMembership>,
}

/// One set's graph node as stored: its immutable meta plus one upward
/// edge per row (`parent_id` is `None` for a set with no parents).
pub(super) struct SetGraphNode {
    pub id: AccountSetId,
    pub journal_id: JournalId,
    pub eventually_consistent: bool,
    pub parent_id: Option<AccountSetId>,
}

/// A single-snapshot read of the whole set graph
/// ([`AccountSetRepo::fetch_set_graph`]).
pub(super) struct SetGraphData {
    pub epoch: i64,
    pub nodes: Vec<SetGraphNode>,
}
