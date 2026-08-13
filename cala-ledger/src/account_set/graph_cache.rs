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
//! `repo.rs`, cache state and snapshot adaptation live here, and pure graph
//! invariants live in `graph_validation.rs`. The call graph is strictly
//! service -> cache -> repo; the repo never calls back up into the cache.
//!
//! Why the split read is safe — the two inputs have opposite write
//! rates:
//!
//! - **Direct memberships** (`cala_account_set_member_accounts`, hot):
//!   always probed live, in-op. The attach fence's correctness
//!   depends on reading these fresh — an account-attach mutates exactly
//!   the rows we still read from the DB, never the cache.
//! - **Set->set edges + per-set metadata** (cold, small; edges mutated
//!   only by `add_member_set` / `add_member_sets` / `remove_member_set`,
//!   all already serialized under the exclusive coarse membership lock): cached
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
//!   or a same-op `add_member_set` / `add_member_sets` bumped it
//!   locally): walk op-locally
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
    graph_validation::{
        has_duplicate_account_membership_paths, validate_set_memberships, AccountMembership,
        SetMembership,
    },
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
    /// parent set -> direct member sets (downward edges). Kept alongside
    /// `parents` so batch validation can find affected components without
    /// repeatedly inverting the snapshot.
    children: HashMap<AccountSetId, Vec<AccountSetId>>,
    /// Every set known at snapshot time. Also the known-set universe for
    /// expansion: an id absent here forces the supplement/fallback path.
    meta: HashMap<AccountSetId, SetMeta>,
}

impl GraphSnapshot {
    fn cold() -> Self {
        Self {
            epoch: COLD_EPOCH,
            parents: HashMap::new(),
            children: HashMap::new(),
            meta: HashMap::new(),
        }
    }

    fn edges_connected_to(&self, members: &[SetMembership]) -> Vec<SetMembership> {
        let mut pending: Vec<_> = members
            .iter()
            .flat_map(|edge| [edge.account_set_id, edge.member_account_set_id])
            .collect();
        let mut visited = HashSet::new();
        let mut edges = HashSet::new();
        while let Some(account_set_id) = pending.pop() {
            if !visited.insert(account_set_id) {
                continue;
            }
            for parent_id in self.parents.get(&account_set_id).into_iter().flatten() {
                edges.insert(SetMembership {
                    account_set_id: *parent_id,
                    member_account_set_id: account_set_id,
                });
                pending.push(*parent_id);
            }
            for member_id in self.children.get(&account_set_id).into_iter().flatten() {
                edges.insert(SetMembership {
                    account_set_id,
                    member_account_set_id: *member_id,
                });
                pending.push(*member_id);
            }
        }
        edges.into_iter().collect()
    }
}

/// Op-local supplement for seed set ids unknown to the shared snapshot
/// (sets created after the last refresh). Never installed.
struct Overlay {
    parents: HashMap<AccountSetId, Vec<AccountSetId>>,
    meta: HashMap<AccountSetId, SetMeta>,
}

struct IndexedNodes {
    parents: HashMap<AccountSetId, Vec<AccountSetId>>,
    children: HashMap<AccountSetId, Vec<AccountSetId>>,
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
        probe_seeds: &[AccountMembership],
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
                .map(|seed| seed.account_set_id)
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
            let IndexedNodes { parents, meta, .. } = Self::index_nodes(nodes);
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
    /// - A same-op `add_member_set` / `add_member_sets` bumps the epoch
    ///   in-op, so the probe reads the bumped value, mismatches every
    ///   shared snapshot, and this method falls back to the SQL walk, which sees the op's
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
        skip(self, op, members),
        fields(pairs = members.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(super) async fn assert_no_double_membership_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        members: &[AccountMembership],
    ) -> Result<(), AccountSetError> {
        let span = tracing::Span::current();

        let distinct_account_ids: Vec<AccountId> = {
            let mut ids: Vec<AccountId> = members.iter().map(|m| m.account_id).collect();
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
                .assert_no_double_membership(op, members)
                .await;
        }

        // Target sets of the new pairs (and, in principle, seed sets from
        // the probe) can be unknown to the snapshot when created after the
        // last refresh — including in this op. Fetch them op-locally as an
        // overlay; never installed.
        let missing: Vec<AccountSetId> = {
            let mut missing: Vec<_> = members
                .iter()
                .chain(probe.seeds.iter())
                .map(|membership| membership.account_set_id)
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
            let IndexedNodes { parents, meta, .. } = Self::index_nodes(nodes);
            Some(Overlay { parents, meta })
        };

        // One lookup carries all three states the walk distinguishes: `None`
        // for a set neither the snapshot nor the overlay knows (defer to SQL),
        // `Some(&[])` for a known root, `Some(parents)` otherwise.
        let parents_of = |set_id: &AccountSetId| -> Option<&[AccountSetId]> {
            let known = snapshot.meta.contains_key(set_id)
                || overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.meta.contains_key(set_id));
            known.then(|| {
                snapshot
                    .parents
                    .get(set_id)
                    .or_else(|| {
                        overlay
                            .as_ref()
                            .and_then(|overlay| overlay.parents.get(set_id))
                    })
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            })
        };

        match has_duplicate_account_membership_paths(members, &probe.seeds, parents_of) {
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
                    .assert_no_double_membership(op, members)
                    .await
            }
        }
    }

    /// Validate a batch of set-to-set memberships against the graph read
    /// inside `op`. The caller must hold the exclusive structure lock so the
    /// graph cannot change between this check and persistence.
    ///
    /// An epoch match proves the cached edges equal the committed graph, so the
    /// affected components are selected in memory. A cold or stale snapshot
    /// falls back to one flat op-local edge read, which also sees same-op
    /// mutations that have already bumped the epoch.
    #[instrument(
        level = "debug",
        name = "account_set.assert_valid_set_memberships",
        skip_all,
        fields(count = members.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(super) async fn assert_valid_set_memberships_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        members: &[SetMembership],
    ) -> Result<(), AccountSetError> {
        let span = tracing::Span::current();
        let snapshot = self.load();
        let epoch = self.inner.repo.fetch_set_graph_epoch_in_op(op).await?;
        let existing_edges = if snapshot.epoch == epoch {
            span.record("path", "memory");
            snapshot.edges_connected_to(members)
        } else {
            span.record(
                "path",
                if snapshot.epoch == COLD_EPOCH {
                    "fallback_cold"
                } else {
                    "fallback_epoch"
                },
            );
            self.spawn_refresh();
            self.inner.repo.fetch_set_membership_edges_in_op(op).await?
        };
        let account_members = self
            .inner
            .repo
            .fetch_affected_account_memberships_in_op(op, &existing_edges, members)
            .await?;
        validate_set_memberships(&existing_edges, members, &account_members)
    }

    fn load(&self) -> Arc<GraphSnapshot> {
        self.inner
            .snapshot
            .read()
            .expect("set_graph_cache snapshot lock poisoned")
            .clone()
    }

    fn index_nodes(nodes: Vec<SetGraphNode>) -> IndexedNodes {
        let mut parents: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut children: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
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
                children.entry(parent_id).or_default().push(node.id);
            }
        }
        IndexedNodes {
            parents,
            children,
            meta,
        }
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
        seeds: &[AccountMembership],
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
        for seed in seeds {
            per_account
                .entry(seed.account_id)
                .or_default()
                .push(seed.account_set_id);
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
        let IndexedNodes {
            parents,
            children,
            meta,
        } = Self::index_nodes(data.nodes);
        let new = GraphSnapshot {
            epoch: data.epoch,
            parents,
            children,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(account_set_id: AccountSetId, member_account_set_id: AccountSetId) -> SetMembership {
        SetMembership {
            account_set_id,
            member_account_set_id,
        }
    }

    #[test]
    fn batch_validation_selects_only_connected_snapshot_edges() {
        let root = AccountSetId::new();
        let branch = AccountSetId::new();
        let leaf = AccountSetId::new();
        let proposed_leaf = AccountSetId::new();
        let unrelated_root = AccountSetId::new();
        let unrelated_leaf = AccountSetId::new();
        let snapshot = GraphSnapshot {
            epoch: 1,
            parents: HashMap::from([
                (branch, vec![root]),
                (leaf, vec![branch]),
                (unrelated_leaf, vec![unrelated_root]),
            ]),
            children: HashMap::from([
                (root, vec![branch]),
                (branch, vec![leaf]),
                (unrelated_root, vec![unrelated_leaf]),
            ]),
            meta: HashMap::new(),
        };

        let edges: HashSet<_> = snapshot
            .edges_connected_to(&[edge(branch, proposed_leaf)])
            .into_iter()
            .collect();

        assert_eq!(
            edges,
            HashSet::from([edge(root, branch), edge(branch, leaf)])
        );
    }

    #[test]
    fn batch_validation_selects_each_component_joined_by_proposed_edges() {
        let left_root = AccountSetId::new();
        let left_leaf = AccountSetId::new();
        let right_root = AccountSetId::new();
        let right_leaf = AccountSetId::new();
        let unrelated_root = AccountSetId::new();
        let unrelated_leaf = AccountSetId::new();
        let snapshot = GraphSnapshot {
            epoch: 1,
            parents: HashMap::from([
                (left_leaf, vec![left_root]),
                (right_leaf, vec![right_root]),
                (unrelated_leaf, vec![unrelated_root]),
            ]),
            children: HashMap::from([
                (left_root, vec![left_leaf]),
                (right_root, vec![right_leaf]),
                (unrelated_root, vec![unrelated_leaf]),
            ]),
            meta: HashMap::new(),
        };

        let edges: HashSet<_> = snapshot
            .edges_connected_to(&[edge(left_leaf, right_root)])
            .into_iter()
            .collect();

        assert_eq!(
            edges,
            HashSet::from([edge(left_root, left_leaf), edge(right_root, right_leaf)])
        );
    }
}
