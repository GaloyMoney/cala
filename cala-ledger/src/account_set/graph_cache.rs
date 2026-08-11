//! Epoch-validated in-process cache of the account-set membership graph.
//!
//! Ancestor resolution on the posting hot path used to be a recursive
//! walk over `cala_account_set_member_account_sets` — measured at
//! ~40 ms/call under heavy posting load, one of the largest DB-time
//! consumers. This module replaces it with **two cheap indexed reads**
//! (a direct-membership
//! probe with a piggybacked epoch check, then one lock statement) plus
//! ancestor expansion in memory, keeping the fused walk+locks SQL only
//! as the rare-path fallback.
//!
//! The same snapshot also backs the attach path's **double-membership
//! check** (`assert_no_double_membership_in_op`): under the
//! account-member lock protocol the SHARED coarse lock excludes set-edge
//! writers for the whole op, so an epoch-matched snapshot can answer the
//! path-uniqueness question in memory — one indexed probe instead of the
//! recursive SQL walk that ran on every account attach (~5x per loan in
//! lana, ~0.45 DB cores at the 2026-08-08 stress soak).
//!
//! Layering: `AccountSets` (service) calls this cache; this cache holds
//! a handle on `AccountSetRepo` and orchestrates — ALL SQL lives in
//! `repo.rs`, all cache state and in-memory graph logic lives here. The
//! call graph is strictly service -> cache -> repo; the repo never
//! calls back up into the cache.
//!
//! Why the split read is safe — the two inputs have opposite write
//! rates:
//!
//! - **Direct memberships** (`cala_account_set_member_accounts`, hot):
//!   always probed live, in-op. The attach fence's correctness
//!   depends on reading these fresh — an account-attach mutates exactly
//!   the rows we still read from the DB, never the cache.
//! - **Set->set edges + per-set metadata** (cold, small; edges mutated
//!   only by `add_member_set` / `remove_member_set`, both already
//!   serialized under the exclusive coarse membership lock): cached
//!   in-process, validated by the `cala_account_set_graph_epoch`
//!   counter read in the SAME statement/snapshot as the probe. Per-set
//!   metadata (`journal_id`, `eventually_consistent`) is immutable
//!   after creation, so only the edge set needs the epoch guard.
//!
//! Equivalence with the single-statement walk: when the probed epoch
//! matches the snapshot's, the cached graph provably equals the
//! committed graph at the probe's snapshot S1, so in-memory expansion
//! resolves exactly what the walk statement would have at S1. A
//! structure op committing after S1 was equally invisible to the
//! atomic walk statement — no correctness delta.
//!
//! Fallbacks (all correctness-critical, all rare) — each runs the
//! original fused walk+locks statement
//! (`AccountSetRepo::walk_mappings_and_lock_in_op`), so a fallback
//! resolution costs one round trip, exactly like the pre-cache path:
//!
//! - **Epoch mismatch** (structure op committed since the last refresh,
//!   or a same-op `add_member_set` bumped it locally): walk op-locally
//!   and separately trigger a background refresh from the **pool**.
//!   Op-local results are NEVER installed into the shared snapshot — an
//!   in-op read can see the op's own uncommitted writes, and installing
//!   them would poison other transactions.
//! - **Unknown seed set id** (set created after the last refresh —
//!   including the same-op create+attach+post pattern, which bumps
//!   no epoch): fetch the missing ids' meta and edges in-op as an
//!   op-local supplement; don't install. Fresh sets normally have no
//!   upward edges (adding one bumps the epoch), so this stays a single
//!   indexed read.
//!
//! Locking: every posting takes exactly ONE Rust-or-SQL-sorted ancestor
//! lock batch — the memory path computes the non-EC `(set, currency)`
//! pairs from cached meta and takes them via
//! `AccountSetRepo::lock_resolved_ancestors_in_op` immediately after
//! expansion; the fallback takes the identical keys inside its walk
//! statement. Locks are never taken from guessed/assumed ancestors in
//! the probe round trip: an advisory-lock wait inside a statement does
//! not refresh that statement's snapshot (the stale-read class the
//! attach fence closes), and a wrong guess would force a second
//! corrective batch, breaking the single-sorted-batch acquisition that
//! poster-vs-poster deadlock-freedom rests on.
//!
//! Multi-instance: each process has its own cache; the per-op epoch
//! check makes cross-instance staleness harmless (worst case = one
//! op-local walk + refresh). A 60 s timer refresh runs as belt and
//! braces against any missed-bump path; correctness never depends on
//! it. Memory: the whole graph is ~thousands of edges + meta — trivial,
//! no eviction needed.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
};
use tracing::instrument;

use crate::primitives::{AccountId, AccountSetId, JournalId};

use super::{
    error::AccountSetError,
    repo::{AccountSetRepo, DirectMembershipProbe, SetGraphNode},
};

/// Interval of the belt-and-braces timer refresh. Correctness never
/// depends on it (the epoch check catches staleness per op); it only
/// bounds how long the fallback path stays hot if no posting triggers
/// a refresh.
const TIMER_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Epoch of the cold-start snapshot. The DB epoch starts at 0 and only
/// increments, so this never matches and the first resolution always
/// takes the op-local fallback (and triggers the installing refresh).
const COLD_EPOCH: i64 = -1;

/// Maximum depth (in set->set edges) of any root-to-leaf membership
/// chain. Rejecting edges past this bound keeps the read-time ancestor
/// walk cheap and terminating. Real hierarchies are <=10 deep; 16 leaves
/// headroom.
pub(super) const MAX_MEMBERSHIP_DEPTH: i32 = 16;

#[derive(Debug, Clone, Copy)]
struct SetMeta {
    journal_id: JournalId,
    eventually_consistent: bool,
}

#[derive(Debug)]
struct GraphSnapshot {
    epoch: i64,
    /// member set -> direct parent sets (upward edges). Sets with no
    /// parents have no entry.
    parents: HashMap<AccountSetId, Vec<AccountSetId>>,
    /// Every set known at snapshot time. Also the known-set universe for
    /// expansion: an id absent here forces the supplement/fallback path.
    meta: HashMap<AccountSetId, SetMeta>,
}

impl GraphSnapshot {
    fn cold() -> Self {
        Self {
            epoch: COLD_EPOCH,
            parents: HashMap::new(),
            meta: HashMap::new(),
        }
    }
}

/// Op-local supplement for seed set ids unknown to the shared snapshot
/// (sets created after the last refresh). Never installed.
struct Overlay {
    parents: HashMap<AccountSetId, Vec<AccountSetId>>,
    meta: HashMap<AccountSetId, SetMeta>,
}

/// The per-process set-graph cache — an internal detail of the
/// `account_set` module, constructed by `AccountSets::new` and shared
/// across its clones. (The streaming EC rollup applier's per-batch
/// resolution deliberately stays a plain walk in
/// `BalanceRepo::fetch_ec_set_mappings`: `Balances` cannot depend on
/// this module without a cycle, and one walk per outbox batch is
/// amortized far below the poster's per-posting rate.)
#[derive(Clone)]
pub(super) struct SetGraphCache {
    inner: Arc<SetGraphCacheInner>,
}

struct SetGraphCacheInner {
    repo: AccountSetRepo,
    /// Immutable snapshot; readers take a brief uncontended read lock,
    /// clone the `Arc`, and release before any await. Refreshes build a
    /// fresh snapshot and swap it in whole.
    snapshot: RwLock<Arc<GraphSnapshot>>,
    /// Single-flight for refreshes: only one runs at a time; concurrent
    /// triggers are dropped (`try_lock`), which is self-healing — a
    /// still-stale snapshot fails the next op's epoch check and triggers
    /// again.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for SetGraphCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetGraphCache").finish_non_exhaustive()
    }
}

impl SetGraphCache {
    pub(super) fn new(repo: AccountSetRepo) -> Self {
        let inner = Arc::new(SetGraphCacheInner {
            repo,
            snapshot: RwLock::new(Arc::new(GraphSnapshot::cold())),
            refresh_lock: tokio::sync::Mutex::new(()),
        });
        // Belt-and-braces timer refresh. Holds only a Weak handle so the
        // task exits (and cannot leak) once every cache clone is dropped.
        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(TIMER_REFRESH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick completes immediately; skip it — the first
            // resolution installs the snapshot via its fallback refresh.
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                if let Err(error) = Self::refresh(&inner).await {
                    tracing::warn!(%error, "set_graph_cache timer refresh failed");
                }
            }
        });
        Self { inner }
    }

    /// Resolve each entry account's ancestor sets for `journal_id` AND
    /// take the poster's per-balance locks on the non-EC ancestors.
    /// Input is the posting's distinct `(account_id, currency)` entry
    /// pairs (parallel arrays), exactly like the pre-cache walk.
    ///
    /// The posting flow reads the direct memberships and the graph epoch
    /// as part of its single read statement, so it arrives here with the
    /// probe already in hand — the probe deliberately takes no locks,
    /// which is what lets it share a statement with the flow's other
    /// reads. Hot path from there: in-memory expansion against the cached
    /// edge graph, then one lock statement for the resolved non-EC
    /// ancestor pairs. Rare paths fall back to the fused walk+locks
    /// statement — see the module doc. Every path reads only the
    /// membership graph, never balance values, and completes its single
    /// ancestor lock batch strictly before any balance data fetch, so the
    /// lock-before-read doctrine holds unchanged.
    ///
    /// `probe_seeds` may cover accounts outside `journal_id` (a batch spanning
    /// journals resolves one journal at a time); expansion filters by journal,
    /// and the lock batch is built from `entry_pairs`, so extra seeds are inert.
    #[instrument(
        level = "debug",
        name = "account_set.resolve_from_probe_in_op",
        skip(self, op, probe_seeds, entry_pairs),
        fields(accounts = entry_pairs.0.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(super) async fn resolve_from_probe_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        probe_epoch: i64,
        probe_seeds: &[(AccountId, AccountSetId)],
        entry_pairs: &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let span = tracing::Span::current();
        let probe = DirectMembershipProbe {
            epoch: probe_epoch,
            seeds: probe_seeds.to_vec(),
        };
        if probe.seeds.is_empty() {
            // No direct memberships => no ancestors, no locks to take.
            span.record("path", "no_memberships");
            return Ok(HashMap::new());
        }

        let snapshot = self.load();
        if snapshot.epoch != probe.epoch {
            // Structure change committed since the last refresh (or this
            // op bumped the epoch itself). Resolve op-locally — the walk
            // sees the op's own writes and takes the ancestor locks in
            // the same statement — and let a background refresh
            // (committed data only) update the shared snapshot.
            span.record(
                "path",
                if snapshot.epoch == COLD_EPOCH {
                    "fallback_cold"
                } else {
                    "fallback_epoch"
                },
            );
            self.spawn_refresh();
            return self
                .inner
                .repo
                .walk_mappings_and_lock_in_op(&mut *op, journal_id, entry_pairs)
                .await;
        }

        // Seed sets are not epoch-guarded (attaching an account bumps
        // nothing), so a set created after the last refresh can appear
        // here — fetch it op-locally as an overlay.
        let missing: Vec<AccountSetId> = {
            let mut missing: Vec<_> = probe
                .seeds
                .iter()
                .map(|(_, set_id)| *set_id)
                .filter(|set_id| !snapshot.meta.contains_key(set_id))
                .collect();
            missing.sort_unstable();
            missing.dedup();
            missing
        };
        let overlay = if missing.is_empty() {
            None
        } else {
            let nodes = self
                .inner
                .repo
                .fetch_set_graph_nodes_in_op(op, &missing)
                .await?;
            let (parents, meta) = Self::index_nodes(nodes);
            Some(Overlay { parents, meta })
        };

        match Self::expand(
            &snapshot,
            overlay.as_ref(),
            journal_id,
            &probe.seeds,
            entry_pairs,
        ) {
            Some((mappings, lock_pairs)) => {
                span.record(
                    "path",
                    if overlay.is_some() {
                        "supplement"
                    } else {
                        "memory"
                    },
                );
                self.inner
                    .repo
                    .lock_resolved_ancestors_in_op(op, journal_id, &lock_pairs)
                    .await?;
                Ok(mappings)
            }
            // Expansion hit a set unknown to snapshot + overlay. With a
            // matching epoch this should be unreachable (every committed
            // edge's endpoints were seen by the refresh; new edges bump
            // the epoch) — but the walk is always correct, so fall back
            // rather than reason about it. No locks were taken yet, so
            // the walk's in-statement batch stays the posting's only one.
            None => {
                span.record("path", "fallback_unknown");
                self.inner
                    .repo
                    .walk_mappings_and_lock_in_op(&mut *op, journal_id, entry_pairs)
                    .await
            }
        }
    }

    /// Path-uniqueness validation for account-member attaches — the
    /// cache-surface form of
    /// [`AccountSetRepo::assert_no_double_membership`], which remains the
    /// rare-path fallback. Returns
    /// [`AccountSetError::MemberAlreadyAdded`] if any `(account, set)`
    /// containment would be reachable via more than one membership path.
    ///
    /// Hot path: one probe statement (the accounts' live direct
    /// memberships + the graph epoch, same snapshot), then an in-memory
    /// path count over the cached edge graph — no recursive SQL. This
    /// runs ~5x per loan in lana (every facility/collateral/deposit
    /// account attach); the SQL walk it replaces was ~0.45 DB cores at
    /// the 2026-08-08 stress soak.
    ///
    /// Why the cached graph is sound to check against, given the caller
    /// holds the account-member lock protocol (SHARED coarse +
    /// EXCLUSIVE per-member, see `ADDVISORY_LOCK_ID` in `repo.rs`):
    ///
    /// - Set->set edges are mutated only under the EXCLUSIVE coarse
    ///   lock, so no edge change can commit between the probe and this
    ///   op's own commit — an epoch match therefore proves the cached
    ///   edge graph equals the committed graph for the *whole op*, not
    ///   just the probe instant. (Stronger than the posting path's
    ///   guarantee, which is snapshot-point only.)
    /// - Direct memberships are read live, in-op: the probe sees this
    ///   op's own earlier attaches, and the per-member EXCLUSIVE lock
    ///   serializes concurrent mutations of the same member — the only
    ///   interleavings that could invalidate the count.
    /// - A same-op `add_member_set` bumps the epoch in-op, so the probe
    ///   reads the bumped value, mismatches every shared snapshot, and
    ///   this method falls back to the SQL walk, which sees the op's
    ///   uncommitted edge.
    ///
    /// Fallbacks (epoch mismatch, unknown set id mid-walk) run the SQL
    /// walk — one round trip, exactly the pre-cache behavior. Set ids
    /// unknown only as *seeds* (fresh sets attached in this op, no epoch
    /// bump) are resolved via the op-local overlay first, mirroring
    /// `fetch_mappings_in_op`'s supplement path.
    #[instrument(
        level = "debug",
        name = "account_set.assert_no_double_membership",
        skip(self, op, account_set_ids, account_ids),
        fields(pairs = account_ids.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(super) async fn assert_no_double_membership_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
        account_ids: &[AccountId],
    ) -> Result<(), AccountSetError> {
        let span = tracing::Span::current();

        let distinct_account_ids: Vec<AccountId> = {
            let mut ids = account_ids.to_vec();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let probe = self
            .inner
            .repo
            .probe_direct_memberships_in_op(op, &distinct_account_ids)
            .await?;

        let snapshot = self.load();
        if snapshot.epoch != probe.epoch {
            span.record(
                "path",
                if snapshot.epoch == COLD_EPOCH {
                    "fallback_cold"
                } else {
                    "fallback_epoch"
                },
            );
            self.spawn_refresh();
            return self
                .inner
                .repo
                .assert_no_double_membership(op, account_set_ids, account_ids)
                .await;
        }

        // Target sets of the new pairs (and, in principle, seed sets from
        // the probe) can be unknown to the snapshot when created after the
        // last refresh — including in this op. Fetch them op-locally as an
        // overlay; never installed.
        let missing: Vec<AccountSetId> = {
            let mut missing: Vec<_> = account_set_ids
                .iter()
                .chain(probe.seeds.iter().map(|(_, set_id)| set_id))
                .copied()
                .filter(|set_id| !snapshot.meta.contains_key(set_id))
                .collect();
            missing.sort_unstable();
            missing.dedup();
            missing
        };
        let overlay = if missing.is_empty() {
            None
        } else {
            let nodes = self
                .inner
                .repo
                .fetch_set_graph_nodes_in_op(op, &missing)
                .await?;
            let (parents, meta) = Self::index_nodes(nodes);
            Some(Overlay { parents, meta })
        };

        match Self::count_membership_paths(
            &snapshot,
            overlay.as_ref(),
            account_set_ids,
            account_ids,
            &probe.seeds,
        ) {
            Some(false) => {
                span.record(
                    "path",
                    if overlay.is_some() {
                        "supplement"
                    } else {
                        "memory"
                    },
                );
                Ok(())
            }
            Some(true) => Err(AccountSetError::MemberAlreadyAdded),
            // A set unknown to snapshot + overlay surfaced mid-walk. With
            // a matching epoch this should be unreachable — but the SQL
            // walk is always correct, so fall back rather than reason
            // about it (mirrors fetch_mappings_in_op).
            None => {
                span.record("path", "fallback_unknown");
                self.inner
                    .repo
                    .assert_no_double_membership(op, account_set_ids, account_ids)
                    .await
            }
        }
    }

    /// The in-memory mirror of the SQL walk's conflict predicate: seed
    /// each account with its new target sets (pair multiplicity
    /// preserved) plus its existing direct memberships, expand every
    /// seed upward over snapshot + overlay counting *paths* (unlike
    /// [`Self::expand`], which dedups via a visited set — reachability
    /// is the wrong question here), and report a conflict as soon as any
    /// `(account, set)` is reached twice.
    ///
    /// Returns `None` if a set id is unknown to snapshot + overlay —
    /// caller falls back to the SQL walk.
    ///
    /// Terminates unconditionally: every pop increments some count and
    /// the second increment of any count returns immediately, so pops
    /// are bounded by (known sets + 1) per account. A corrupted cyclic
    /// graph revisits a set and reports a conflict — a conservative
    /// reject, where the SQL walk's unbounded UNION ALL recursion would
    /// not terminate at all.
    fn count_membership_paths(
        snapshot: &GraphSnapshot,
        overlay: Option<&Overlay>,
        new_set_ids: &[AccountSetId],
        new_account_ids: &[AccountId],
        existing_seeds: &[(AccountId, AccountSetId)],
    ) -> Option<bool> {
        let known = |set_id: &AccountSetId| -> bool {
            snapshot.meta.contains_key(set_id)
                || overlay.is_some_and(|o| o.meta.contains_key(set_id))
        };
        let parents_of = |set_id: &AccountSetId| -> &[AccountSetId] {
            snapshot
                .parents
                .get(set_id)
                .or_else(|| overlay.and_then(|o| o.parents.get(set_id)))
                .map(Vec::as_slice)
                .unwrap_or(&[])
        };

        let mut per_account: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        for (set_id, account_id) in new_set_ids.iter().zip(new_account_ids) {
            per_account.entry(*account_id).or_default().push(*set_id);
        }
        for (account_id, set_id) in existing_seeds {
            per_account.entry(*account_id).or_default().push(*set_id);
        }

        for seeds in per_account.into_values() {
            let mut path_counts: HashMap<AccountSetId, u32> = HashMap::new();
            let mut queue: VecDeque<AccountSetId> = seeds.into();
            while let Some(set_id) = queue.pop_front() {
                if !known(&set_id) {
                    return None;
                }
                let count = path_counts.entry(set_id).or_default();
                *count += 1;
                if *count > 1 {
                    return Some(true);
                }
                queue.extend(parents_of(&set_id));
            }
        }
        Some(false)
    }

    /// Validate a batch of set-to-set memberships against the graph read
    /// inside `op`. The caller must hold the exclusive structure lock so the
    /// graph cannot change between this check and persistence.
    pub(super) async fn assert_valid_set_memberships_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        members: &[(AccountSetId, AccountSetId)],
    ) -> Result<(), AccountSetError> {
        let (existing_edges, account_members) = self
            .inner
            .repo
            .fetch_set_membership_validation_data_in_op(op, members)
            .await?;
        Self::validate_set_memberships(&existing_edges, members, &account_members)
    }

    fn validate_set_memberships(
        existing_edges: &[(AccountSetId, AccountSetId)],
        proposed_edges: &[(AccountSetId, AccountSetId)],
        account_members: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetError> {
        let mut nodes = HashSet::new();
        let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();

        for (account_set_id, member_account_set_id) in existing_edges.iter().chain(proposed_edges) {
            nodes.insert(*account_set_id);
            nodes.insert(*member_account_set_id);
            adjacency
                .entry(*account_set_id)
                .or_default()
                .push(*member_account_set_id);
            *indegree.entry(*member_account_set_id).or_default() += 1;
            indegree.entry(*account_set_id).or_default();
        }
        for (account_set_id, _) in account_members {
            nodes.insert(*account_set_id);
            indegree.entry(*account_set_id).or_default();
        }

        let mut queue: VecDeque<AccountSetId> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut topological_order = Vec::with_capacity(nodes.len());
        while let Some(account_set_id) = queue.pop_front() {
            topological_order.push(account_set_id);
            if let Some(children) = adjacency.get(&account_set_id) {
                for child in children {
                    let degree = indegree
                        .get_mut(child)
                        .expect("every child must have an indegree");
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(*child);
                    }
                }
            }
        }

        if topological_order.len() != nodes.len() {
            let (account_set_id, member_account_set_id) = proposed_edges
                .iter()
                .find(|(account_set_id, member_account_set_id)| {
                    account_set_id == member_account_set_id
                        || Self::graph_has_path(&adjacency, *member_account_set_id, *account_set_id)
                })
                .copied()
                .unwrap_or(proposed_edges[0]);
            return Err(AccountSetError::MembershipCycleDetected {
                account_set_id,
                member_account_set_id,
            });
        }

        // In topological order, every parent's ancestor set is complete before
        // its contribution reaches a child. An overlap means the child has two
        // paths to the same ancestor. Hash-set union caps work at O(V^2) in the
        // worst case instead of enumerating exponentially many paths.
        let mut ancestors: HashMap<AccountSetId, HashSet<AccountSetId>> = HashMap::new();
        let mut depth_from_root: HashMap<AccountSetId, i32> = HashMap::new();
        for account_set_id in &topological_order {
            let mut contribution = ancestors.get(account_set_id).cloned().unwrap_or_default();
            contribution.insert(*account_set_id);
            let parent_depth = *depth_from_root.get(account_set_id).unwrap_or(&0);

            if let Some(children) = adjacency.get(account_set_id) {
                for child in children {
                    let child_ancestors = ancestors.entry(*child).or_default();
                    if !child_ancestors.is_disjoint(&contribution) {
                        return Err(AccountSetError::MemberAlreadyAdded);
                    }
                    child_ancestors.extend(contribution.iter().copied());
                    depth_from_root
                        .entry(*child)
                        .and_modify(|depth| *depth = (*depth).max(parent_depth + 1))
                        .or_insert(parent_depth + 1);
                }
            }
        }

        let mut account_paths = HashSet::new();
        for (account_set_id, account_id) in account_members {
            if !account_paths.insert((*account_set_id, *account_id)) {
                return Err(AccountSetError::MemberAlreadyAdded);
            }
            if let Some(containers) = ancestors.get(account_set_id) {
                for container in containers {
                    if !account_paths.insert((*container, *account_id)) {
                        return Err(AccountSetError::MemberAlreadyAdded);
                    }
                }
            }
        }

        let max_depth = depth_from_root.values().copied().max().unwrap_or(0);
        if max_depth > MAX_MEMBERSHIP_DEPTH {
            let (index, depth) = Self::first_depth_overflow(existing_edges, proposed_edges);
            let (account_set_id, member_account_set_id) = proposed_edges[index];
            return Err(AccountSetError::MembershipDepthExceeded {
                account_set_id,
                member_account_set_id,
                depth,
                max: MAX_MEMBERSHIP_DEPTH,
            });
        }

        Ok(())
    }

    fn graph_has_path(
        adjacency: &HashMap<AccountSetId, Vec<AccountSetId>>,
        from: AccountSetId,
        to: AccountSetId,
    ) -> bool {
        let mut pending = vec![from];
        let mut visited = HashSet::new();
        while let Some(current) = pending.pop() {
            if current == to {
                return true;
            }
            if visited.insert(current) {
                pending.extend(adjacency.get(&current).into_iter().flatten().copied());
            }
        }
        false
    }

    fn first_depth_overflow(
        existing_edges: &[(AccountSetId, AccountSetId)],
        proposed_edges: &[(AccountSetId, AccountSetId)],
    ) -> (usize, i32) {
        let mut low = 1;
        let mut high = proposed_edges.len();
        while low < high {
            let middle = (low + high) / 2;
            if Self::graph_max_depth(existing_edges, &proposed_edges[..middle])
                > MAX_MEMBERSHIP_DEPTH
            {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        (
            low - 1,
            Self::graph_max_depth(existing_edges, &proposed_edges[..low]),
        )
    }

    fn graph_max_depth(
        existing_edges: &[(AccountSetId, AccountSetId)],
        proposed_edges: &[(AccountSetId, AccountSetId)],
    ) -> i32 {
        let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();
        for (account_set_id, member_account_set_id) in existing_edges.iter().chain(proposed_edges) {
            adjacency
                .entry(*account_set_id)
                .or_default()
                .push(*member_account_set_id);
            *indegree.entry(*member_account_set_id).or_default() += 1;
            indegree.entry(*account_set_id).or_default();
        }

        let mut queue: VecDeque<AccountSetId> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut depths = HashMap::new();
        let mut max_depth = 0;
        while let Some(account_set_id) = queue.pop_front() {
            let parent_depth = *depths.get(&account_set_id).unwrap_or(&0);
            if let Some(children) = adjacency.get(&account_set_id) {
                for child in children {
                    let child_depth = parent_depth + 1;
                    depths
                        .entry(*child)
                        .and_modify(|depth| *depth = (*depth).max(child_depth))
                        .or_insert(child_depth);
                    max_depth = max_depth.max(child_depth);
                    let degree = indegree
                        .get_mut(child)
                        .expect("every child must have an indegree");
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(*child);
                    }
                }
            }
        }
        max_depth
    }

    fn load(&self) -> Arc<GraphSnapshot> {
        self.inner
            .snapshot
            .read()
            .expect("set_graph_cache snapshot lock poisoned")
            .clone()
    }

    fn index_nodes(
        nodes: Vec<SetGraphNode>,
    ) -> (
        HashMap<AccountSetId, Vec<AccountSetId>>,
        HashMap<AccountSetId, SetMeta>,
    ) {
        let mut parents: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut meta = HashMap::new();
        for node in nodes {
            meta.insert(
                node.id,
                SetMeta {
                    journal_id: node.journal_id,
                    eventually_consistent: node.eventually_consistent,
                },
            );
            if let Some(parent_id) = node.parent_id {
                parents.entry(node.id).or_default().push(parent_id);
            }
        }
        (parents, meta)
    }

    /// In-memory BFS expansion of the probed seeds over snapshot +
    /// overlay. Mirrors the walk SQL exactly: ancestors are walked
    /// across ALL journals (a set in another journal is walked
    /// *through*), and only sets whose `journal_id` matches are included
    /// in the result. Also computes the fallback CTE's lock list — the
    /// distinct non-EC `(ancestor, currency)` pairs with per-leaf
    /// currency semantics (each leaf's currencies propagate to exactly
    /// *its own* ancestors), returned deduped and Rust-sorted for
    /// canonical acquisition. Returns `None` if any set id is unknown —
    /// caller falls back to the walk.
    #[allow(clippy::type_complexity)]
    fn expand<'c>(
        snapshot: &GraphSnapshot,
        overlay: Option<&Overlay>,
        journal_id: JournalId,
        seeds: &[(AccountId, AccountSetId)],
        (entry_account_ids, entry_currencies): &(Vec<AccountId>, Vec<&'c str>),
    ) -> Option<(
        HashMap<AccountId, Vec<AccountSetId>>,
        (Vec<AccountSetId>, Vec<&'c str>),
    )> {
        let meta_of = |set_id: &AccountSetId| -> Option<SetMeta> {
            snapshot
                .meta
                .get(set_id)
                .or_else(|| overlay.and_then(|o| o.meta.get(set_id)))
                .copied()
        };
        let parents_of = |set_id: &AccountSetId| -> &[AccountSetId] {
            snapshot
                .parents
                .get(set_id)
                .or_else(|| overlay.and_then(|o| o.parents.get(set_id)))
                .map(Vec::as_slice)
                .unwrap_or(&[])
        };

        let mut per_account: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        for (account_id, set_id) in seeds {
            per_account.entry(*account_id).or_default().push(*set_id);
        }

        let mut mappings = HashMap::new();
        let mut non_ec: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        for (account_id, seed_sets) in per_account {
            let mut visited: HashSet<AccountSetId> = HashSet::new();
            let mut queue: VecDeque<AccountSetId> = seed_sets.into();
            let mut ancestors = Vec::new();
            let mut non_ec_ancestors = Vec::new();
            while let Some(set_id) = queue.pop_front() {
                if !visited.insert(set_id) {
                    continue;
                }
                let meta = meta_of(&set_id)?;
                if meta.journal_id == journal_id {
                    ancestors.push(set_id);
                    if !meta.eventually_consistent {
                        non_ec_ancestors.push(set_id);
                    }
                }
                queue.extend(parents_of(&set_id));
            }
            if !non_ec_ancestors.is_empty() {
                non_ec.insert(account_id, non_ec_ancestors);
            }
            if !ancestors.is_empty() {
                mappings.insert(account_id, ancestors);
            }
        }

        let mut lock_pairs: Vec<(AccountSetId, &str)> = entry_account_ids
            .iter()
            .zip(entry_currencies.iter())
            .flat_map(|(account_id, currency)| {
                non_ec
                    .get(account_id)
                    .into_iter()
                    .flatten()
                    .map(move |set_id| (*set_id, *currency))
            })
            .collect();
        lock_pairs.sort_unstable();
        lock_pairs.dedup();

        Some((mappings, lock_pairs.into_iter().unzip()))
    }

    fn spawn_refresh(&self) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            if let Err(error) = Self::refresh(&inner).await {
                tracing::warn!(%error, "set_graph_cache refresh failed");
            }
        });
    }

    /// Rebuild the shared snapshot from **committed** data (the repo's
    /// pool-side read — never an op executor, so uncommitted writes
    /// can't leak in). The epoch-monotonic install guard makes a slow
    /// refresh racing a faster one harmless.
    #[instrument(
        level = "debug",
        name = "cala_ledger.set_graph_cache.refresh",
        skip_all,
        fields(epoch = tracing::field::Empty, sets = tracing::field::Empty),
        err(level = "warn")
    )]
    async fn refresh(inner: &SetGraphCacheInner) -> Result<(), AccountSetError> {
        let Ok(_guard) = inner.refresh_lock.try_lock() else {
            return Ok(());
        };
        let data = inner.repo.fetch_set_graph().await?;
        let (parents, meta) = Self::index_nodes(data.nodes);
        let new = GraphSnapshot {
            epoch: data.epoch,
            parents,
            meta,
        };
        tracing::Span::current().record("epoch", new.epoch);
        tracing::Span::current().record("sets", new.meta.len());

        let mut current = inner
            .snapshot
            .write()
            .expect("set_graph_cache snapshot lock poisoned");
        // Same-epoch reinstall is fine (the graph is identical modulo
        // sets created since — a superset); an older epoch never
        // overwrites a newer one.
        if new.epoch >= current.epoch {
            *current = Arc::new(new);
        }
        Ok(())
    }
}
