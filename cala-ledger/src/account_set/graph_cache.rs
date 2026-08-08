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
//! Layering: `AccountSets` (service) calls this cache; this cache holds
//! a handle on `AccountSetRepo` and orchestrates — ALL SQL lives in
//! `repo.rs`, all cache state and in-memory graph logic lives here.
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
    repo::{AccountSetRepo, SetGraphNode},
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
    /// Hot path: one probe statement (live direct memberships + epoch in
    /// the same snapshot), in-memory expansion against the cached edge
    /// graph, then one lock statement for the resolved non-EC ancestor
    /// pairs. Rare paths fall back to the fused walk+locks statement —
    /// see the module doc. Every path reads only the membership graph,
    /// never balance values, and completes its single ancestor lock
    /// batch strictly before `find_for_update`'s balance data fetch, so
    /// the lock-before-read doctrine holds unchanged.
    #[instrument(
        level = "debug",
        name = "account_set.fetch_mappings_in_op",
        skip(self, op, entry_pairs),
        fields(accounts = entry_pairs.0.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(super) async fn fetch_mappings_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        entry_pairs: &(Vec<AccountId>, Vec<&str>),
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        let span = tracing::Span::current();

        let account_ids: Vec<AccountId> = {
            let mut ids = entry_pairs.0.clone();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let Some(probe) = self
            .inner
            .repo
            .probe_direct_memberships_in_op(op, &account_ids)
            .await?
        else {
            // No direct memberships => no ancestors, no locks to take.
            span.record("path", "no_memberships");
            return Ok(HashMap::new());
        };

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
            let (parents, meta) = index_nodes(nodes);
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

    fn load(&self) -> Arc<GraphSnapshot> {
        self.inner
            .snapshot
            .read()
            .expect("set_graph_cache snapshot lock poisoned")
            .clone()
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
        let (parents, meta) = index_nodes(data.nodes);
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
