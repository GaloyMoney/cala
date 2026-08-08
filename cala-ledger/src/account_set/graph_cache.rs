//! Epoch-validated in-process cache of the account-set membership graph.
//!
//! Ancestor resolution on the posting hot path used to be a recursive
//! walk over `cala_account_set_member_account_sets` (#814) — measured at
//! ~40 ms/call at lana scale, the #2 DB-time consumer. This module
//! replaces it with **two cheap indexed reads** (a direct-membership
//! probe with a piggybacked epoch check) plus in-memory ancestor
//! expansion, keeping the walk SQL only as the rare-path fallback.
//!
//! Why this split is safe — the two inputs have opposite write rates:
//!
//! - **Direct memberships** (`cala_account_set_member_accounts`, hot):
//!   always probed live, in-op. The #816 attach fence's correctness
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
//! Fallbacks (all correctness-critical, all rare):
//!
//! - **Epoch mismatch** (structure op committed since the last refresh,
//!   or a same-op `add_member_set` bumped it locally): run the walk
//!   op-locally and separately trigger a background refresh from the
//!   **pool**. Op-local results are NEVER installed into the shared
//!   snapshot — an in-op read can see the op's own uncommitted writes,
//!   and installing them would poison other transactions.
//! - **Unknown seed set id** (set created after the last refresh —
//!   including lana's same-op create+attach+post pattern, which bumps
//!   no epoch): fetch the missing ids' meta and edges in-op as an
//!   op-local supplement; don't install. Fresh sets normally have no
//!   upward edges (adding one bumps the epoch), so this stays a single
//!   indexed read.
//!
//! Multi-instance: each process has its own cache; the per-op epoch
//! check makes cross-instance staleness harmless (worst case = one
//! op-local walk + refresh). A 60 s timer refresh runs as belt and
//! braces against any missed-bump path; correctness never depends on
//! it. Memory: the whole graph is ~thousands of edges + meta — trivial,
//! no eviction needed.

use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
};
use tracing::instrument;

use crate::primitives::{AccountId, AccountSetId, JournalId};

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

/// Result of one ancestor resolution — the shared output of the memory
/// path and the walk fallback, consumed by both the poster
/// (`AccountSets::fetch_mappings_in_op`) and the streaming EC rollup
/// applier (`Balances::apply_ec_rollup_in_op`).
pub(crate) struct GraphResolution {
    /// account -> its ancestor sets in the resolution journal (all
    /// consistency modes). Accounts with no ancestors in the journal
    /// have no entry. Per-account: each leaf's ancestors are exactly
    /// *its own* — this is what per-leaf-currency lock computation and
    /// balance fan-out key off.
    pub mappings: HashMap<AccountId, Vec<AccountSetId>>,
    /// The eventually-consistent subset of every set appearing in
    /// `mappings`. Posters lock and fan out inline over the complement;
    /// the streaming rollup owns these.
    pub ec_sets: HashSet<AccountSetId>,
}

impl GraphResolution {
    fn empty() -> Self {
        Self {
            mappings: HashMap::new(),
            ec_sets: HashSet::new(),
        }
    }
}

/// Op-local supplement for seed set ids unknown to the shared snapshot
/// (sets created after the last refresh). Never installed.
struct Overlay {
    parents: HashMap<AccountSetId, Vec<AccountSetId>>,
    meta: HashMap<AccountSetId, SetMeta>,
}

/// Shared handle to the per-process set-graph cache. Owned by
/// `CalaLedger::init` and cloned into `AccountSets` (poster resolution)
/// and `Balances` (EC rollup applier resolution).
#[derive(Clone)]
pub(crate) struct SetGraphCache {
    inner: Arc<SetGraphCacheInner>,
}

struct SetGraphCacheInner {
    pool: PgPool,
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
    pub(crate) fn new(pool: &PgPool) -> Self {
        let inner = Arc::new(SetGraphCacheInner {
            pool: pool.clone(),
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

    /// Resolve each account's ancestor sets for `journal_id`.
    ///
    /// One statement on the op executor — the direct-membership probe
    /// with the graph epoch piggybacked into the same snapshot S1 —
    /// then, when the cached snapshot matches S1's epoch and all seed
    /// sets are known, pure in-memory expansion. Otherwise falls back
    /// op-locally (see the module doc). Reads only the membership
    /// graph, never balance values, so callers may take balance locks
    /// derived from the result *after* this returns while preserving
    /// lock-before-read (see
    /// `BalanceRepo::lock_ancestor_balances_in_op`).
    #[instrument(
        level = "debug",
        name = "cala_ledger.set_graph_cache.resolve",
        skip(self, op, account_ids),
        fields(accounts = account_ids.len(), path = tracing::field::Empty),
        err(level = "warn")
    )]
    pub(crate) async fn resolve_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<GraphResolution, sqlx::Error> {
        let span = tracing::Span::current();
        let rows = sqlx::query!(
            r#"
            SELECT
                m.member_account_id AS "account_id!: AccountId",
                m.account_set_id AS "set_id!: AccountSetId",
                (SELECT epoch FROM cala_account_set_graph_epoch) AS "epoch!"
            FROM cala_account_set_member_accounts m
            WHERE m.member_account_id = ANY($1)
            "#,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        // No direct memberships => no ancestors; the epoch is irrelevant.
        let Some(epoch) = rows.first().map(|row| row.epoch) else {
            span.record("path", "no_memberships");
            return Ok(GraphResolution::empty());
        };

        let snapshot = self.load();
        if snapshot.epoch != epoch {
            // Structure change committed since the last refresh (or this
            // op bumped the epoch itself). Resolve op-locally — the walk
            // sees S1-adjacent state including the op's own writes — and
            // let a background refresh (committed data only) update the
            // shared snapshot.
            span.record(
                "path",
                if snapshot.epoch == COLD_EPOCH {
                    "fallback_cold"
                } else {
                    "fallback_epoch"
                },
            );
            self.spawn_refresh();
            return Self::walk_in_op(op, journal_id, account_ids).await;
        }

        let seeds: Vec<(AccountId, AccountSetId)> = rows
            .into_iter()
            .map(|row| (row.account_id, row.set_id))
            .collect();

        // Seed sets are not epoch-guarded (attaching an account bumps
        // nothing), so a set created after the last refresh can appear
        // here — fetch it op-locally as an overlay.
        let missing: Vec<AccountSetId> = {
            let mut missing: Vec<_> = seeds
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
            Some(Self::fetch_overlay_in_op(op, &missing).await?)
        };

        match Self::expand(&snapshot, overlay.as_ref(), journal_id, &seeds) {
            Some(resolution) => {
                span.record(
                    "path",
                    if overlay.is_some() {
                        "supplement"
                    } else {
                        "memory"
                    },
                );
                Ok(resolution)
            }
            // Expansion hit a set unknown to snapshot + overlay. With a
            // matching epoch this should be unreachable (every committed
            // edge's endpoints were seen by the refresh; new edges bump
            // the epoch) — but the walk is always correct, so fall back
            // rather than reason about it.
            None => {
                span.record("path", "fallback_unknown");
                Self::walk_in_op(op, journal_id, account_ids).await
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

    /// In-memory BFS expansion of `seeds` over snapshot + overlay.
    /// Mirrors the walk SQL exactly: ancestors are walked across ALL
    /// journals (a set in another journal is walked *through*), and only
    /// sets whose `journal_id` matches are included in the result.
    /// Returns `None` if any set id is unknown — caller falls back.
    fn expand(
        snapshot: &GraphSnapshot,
        overlay: Option<&Overlay>,
        journal_id: JournalId,
        seeds: &[(AccountId, AccountSetId)],
    ) -> Option<GraphResolution> {
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
        let mut ec_sets = HashSet::new();
        for (account_id, seed_sets) in per_account {
            let mut visited: HashSet<AccountSetId> = HashSet::new();
            let mut queue: VecDeque<AccountSetId> = seed_sets.into();
            let mut ancestors = Vec::new();
            while let Some(set_id) = queue.pop_front() {
                if !visited.insert(set_id) {
                    continue;
                }
                let meta = meta_of(&set_id)?;
                if meta.journal_id == journal_id {
                    ancestors.push(set_id);
                    if meta.eventually_consistent {
                        ec_sets.insert(set_id);
                    }
                }
                queue.extend(parents_of(&set_id));
            }
            if !ancestors.is_empty() {
                mappings.insert(account_id, ancestors);
            }
        }
        Some(GraphResolution { mappings, ec_sets })
    }

    /// The rare-path resolution: the #814 recursive walk, minus the
    /// old locks CTE (ancestor locks are computed app-side from the
    /// resolution — see `BalanceRepo::lock_ancestor_balances_in_op`).
    /// Runs on the op executor so it sees the op's own uncommitted
    /// membership writes, exactly like the pre-cache statement did.
    async fn walk_in_op(
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<GraphResolution, sqlx::Error> {
        // UNION (not UNION ALL) dedups and keeps the walk terminating
        // even if a stray edge slipped past the write-side cycle check.
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE seed AS (
                SELECT m.member_account_id AS account_id, m.account_set_id
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
            )
            SELECT
                a.account_id AS "account_id!: AccountId",
                a.account_set_id AS "set_id!: AccountSetId",
                acc.eventually_consistent AS "eventually_consistent!"
            FROM ancestors a
            JOIN cala_account_sets s
              ON s.id = a.account_set_id AND s.journal_id = $1
            JOIN cala_accounts acc
              ON acc.id = a.account_set_id
            "#,
            journal_id as JournalId,
            account_ids as &[AccountId],
        )
        .fetch_all(op.as_executor())
        .await?;

        let mut mappings: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
        let mut ec_sets = HashSet::new();
        for row in rows {
            mappings.entry(row.account_id).or_default().push(row.set_id);
            if row.eventually_consistent {
                ec_sets.insert(row.set_id);
            }
        }
        Ok(GraphResolution { mappings, ec_sets })
    }

    /// Fetch meta + upward edges for seed set ids unknown to the shared
    /// snapshot, on the op executor (sees the op's own uncommitted set
    /// creations). The result stays op-local — see the module doc.
    async fn fetch_overlay_in_op(
        op: &mut impl es_entity::AtomicOperation,
        set_ids: &[AccountSetId],
    ) -> Result<Overlay, sqlx::Error> {
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

        let mut overlay = Overlay {
            parents: HashMap::new(),
            meta: HashMap::new(),
        };
        for row in rows {
            overlay.meta.insert(
                row.set_id,
                SetMeta {
                    journal_id: row.journal_id,
                    eventually_consistent: row.eventually_consistent,
                },
            );
            if let Some(parent_id) = row.parent_id {
                overlay
                    .parents
                    .entry(row.set_id)
                    .or_default()
                    .push(parent_id);
            }
        }
        Ok(overlay)
    }

    fn spawn_refresh(&self) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            if let Err(error) = Self::refresh(&inner).await {
                tracing::warn!(%error, "set_graph_cache refresh failed");
            }
        });
    }

    /// Rebuild the shared snapshot from **committed** data (the pool —
    /// never an op executor, so uncommitted writes can't leak in). One
    /// statement, so epoch and graph come from a single snapshot. The
    /// epoch-monotonic install guard makes a slow refresh racing a
    /// faster one harmless.
    #[instrument(
        level = "debug",
        name = "cala_ledger.set_graph_cache.refresh",
        skip_all,
        fields(epoch = tracing::field::Empty, sets = tracing::field::Empty),
        err(level = "warn")
    )]
    async fn refresh(inner: &SetGraphCacheInner) -> Result<(), sqlx::Error> {
        let Ok(_guard) = inner.refresh_lock.try_lock() else {
            return Ok(());
        };
        // Anchoring on the always-present epoch row guarantees >=1 row
        // even with zero account sets.
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
        .fetch_all(&inner.pool)
        .await?;

        let epoch = rows.first().map(|row| row.epoch).unwrap_or(COLD_EPOCH);
        let mut parents: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut meta = HashMap::new();
        for row in rows {
            let (Some(set_id), Some(journal_id), Some(eventually_consistent)) =
                (row.set_id, row.journal_id, row.eventually_consistent)
            else {
                continue;
            };
            meta.insert(
                set_id,
                SetMeta {
                    journal_id,
                    eventually_consistent,
                },
            );
            if let Some(parent_id) = row.parent_id {
                parents.entry(set_id).or_default().push(parent_id);
            }
        }
        let new = GraphSnapshot {
            epoch,
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
