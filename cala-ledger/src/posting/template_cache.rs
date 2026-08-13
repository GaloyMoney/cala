//! Per-process cache of `code -> (id, version, body)` for the posting flow.
//!
//! The steady state resolves a template with zero round trips; the DB is
//! touched only on a miss (a code this process has never posted) or a forced
//! refresh. Correctness never depends on freshness: every posting flow
//! re-reads the authoritative `(id, MAX(sequence))` for the codes it used in
//! its fence statement ([`TemplateCache::check`]) and re-prepares via
//! [`TemplateCache::refresh_in_op`] on a mismatch. The cache only removes the
//! round trip that lookup would otherwise have cost on its own.
//!
//! Why optimistic-then-verify rather than probe-then-prepare: the fence
//! statement needs the posting's entry accounts, and the entry accounts are
//! only known once the template body has been CEL-evaluated. Probing the
//! version first would therefore cost a round trip that cannot overlap with
//! anything. Preparing from cache and riding the version check on the fence
//! statement gets the same guarantee for free, at the cost of a rare
//! re-preparation.
//!
//! Layering (mirrors `account_set::graph_cache`): `Postings` (service) calls
//! this cache; the cache holds a handle on [`PostingRepo`] and decides
//! cache-or-DB itself. The call graph is strictly service -> cache -> repo;
//! the repo never calls back up into the cache.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use cala_types::tx_template::TxTemplateValues;

use crate::{
    primitives::TxTemplateId,
    tx_template::{error::TxTemplateError, TxTemplateEvent},
};

use super::repo::PostingRepo;

/// A template body resolved at a known version.
#[derive(Clone)]
pub(super) struct ResolvedTemplate {
    pub id: TxTemplateId,
    pub version: i32,
    pub values: Arc<TxTemplateValues>,
}

#[derive(Clone)]
pub(super) struct TemplateCache {
    inner: Arc<TemplateCacheInner>,
}

struct TemplateCacheInner {
    repo: PostingRepo,
    /// Immutable snapshot map: readers take one brief uncontended read lock
    /// per resolve — not per code — clone the `Arc`, and release before doing
    /// any lookups (the same discipline as `SetGraphCache::load`). Writers
    /// copy the map, insert, and swap the `Arc` whole; installs happen once
    /// per code per process plus the rare staleness refresh, so the
    /// copy-on-write cost is irrelevant.
    snapshot: RwLock<Arc<HashMap<String, ResolvedTemplate>>>,
}

impl TemplateCache {
    pub(super) fn new(repo: PostingRepo) -> Self {
        Self {
            inner: Arc::new(TemplateCacheInner {
                repo,
                snapshot: RwLock::new(Arc::new(HashMap::new())),
            }),
        }
    }

    /// Resolve `codes` from the cache, fetching — and installing — only the
    /// ones this process has never seen. The versions returned are *assumed*,
    /// not verified; the flow's fence statement re-checks them.
    pub(super) async fn resolve_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        codes: &[String],
    ) -> Result<HashMap<String, ResolvedTemplate>, TxTemplateError> {
        let snapshot = self.load();
        let mut used = HashMap::new();
        let mut missing = Vec::new();
        for code in codes {
            match snapshot.get(code) {
                Some(resolved) => {
                    used.insert(code.clone(), resolved.clone());
                }
                None => missing.push(code.clone()),
            }
        }
        if !missing.is_empty() {
            used.extend(self.fetch_and_install(op, &missing).await?);
        }
        Ok(used)
    }

    /// Re-resolve `codes` from the database unconditionally, replacing the
    /// cached entries.
    ///
    /// This is the staleness path — the fence observed a version other than
    /// the one preparation used — so the cache must NOT be consulted: what it
    /// holds for these codes is exactly what was reported stale.
    pub(super) async fn refresh_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        codes: &[String],
    ) -> Result<HashMap<String, ResolvedTemplate>, TxTemplateError> {
        self.fetch_and_install(op, codes).await
    }

    /// Assert that the versions this flow prepared against are the versions
    /// the fence statement observed.
    ///
    /// `Ok(())` means every body used is current and the flow may proceed.
    /// `Err(codes)` carries the codes that moved and must be re-resolved via
    /// [`Self::refresh_in_op`] before re-preparing.
    ///
    /// A code missing from `observed` means the template was deleted between
    /// preparation and the fence; it is reported stale so the re-resolution
    /// path surfaces the proper `NotFound`.
    pub(super) fn assert_up_to_date(
        used: &HashMap<String, ResolvedTemplate>,
        observed: &HashMap<String, (TxTemplateId, i32)>,
    ) -> Result<(), Vec<String>> {
        let stale: Vec<String> = used
            .iter()
            .filter(|(code, resolved)| {
                observed.get(*code) != Some(&(resolved.id, resolved.version))
            })
            .map(|(code, _)| code.clone())
            .collect();
        if stale.is_empty() {
            Ok(())
        } else {
            Err(stale)
        }
    }

    async fn fetch_and_install(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        codes: &[String],
    ) -> Result<HashMap<String, ResolvedTemplate>, TxTemplateError> {
        let mut fetched = self.inner.repo.resolve_templates_in_op(op, codes).await?;
        let mut resolved = HashMap::with_capacity(codes.len());
        for code in codes {
            let Some((id, version, event)) = fetched.remove(code) else {
                return Err(TxTemplateError::NotFound);
            };
            let event: TxTemplateEvent = serde_json::from_value(event)?;
            resolved.insert(
                code.clone(),
                ResolvedTemplate {
                    id,
                    version,
                    values: Arc::new(event.into_values()),
                },
            );
        }
        self.install(&resolved);
        Ok(resolved)
    }

    fn load(&self) -> Arc<HashMap<String, ResolvedTemplate>> {
        self.inner
            .snapshot
            .read()
            .expect("template cache poisoned")
            .clone()
    }

    fn install(&self, resolved: &HashMap<String, ResolvedTemplate>) {
        let mut guard = self
            .inner
            .snapshot
            .write()
            .expect("template cache poisoned");
        let mut next = HashMap::clone(&guard);
        for (code, template) in resolved {
            next.insert(code.clone(), template.clone());
        }
        *guard = Arc::new(next);
    }
}
