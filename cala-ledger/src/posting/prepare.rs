//! Client-side preparation: template resolution and CEL evaluation.
//!
//! Everything here runs **before** the posting flow's first statement, from
//! an in-process cache. The DB is touched only on a cache miss (a code this
//! process has never posted) — the steady state resolves a template with zero
//! round trips, and the fence statement re-checks the version it used (see
//! [`TemplateCache::check`]).
//!
//! Why optimistic-then-verify rather than probe-then-prepare: the fence
//! statement needs the posting's entry accounts, and the entry accounts are
//! only known once the template body has been CEL-evaluated. Probing the
//! version first would therefore cost a round trip that cannot overlap with
//! anything. Preparing from cache and riding the version check on the fence
//! statement gets the same guarantee for free, at the cost of a rare
//! re-preparation.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

use cala_types::tx_template::TxTemplateValues;

use crate::{
    entry::NewEntry,
    primitives::*,
    transaction::NewTransaction,
    tx_template::{error::TxTemplateError, Params},
};

/// A template body resolved at a known version.
#[derive(Clone)]
pub(super) struct ResolvedTemplate {
    pub id: TxTemplateId,
    pub version: i32,
    pub values: Arc<TxTemplateValues>,
}

/// Per-process cache of `code -> (id, version, body)`.
///
/// Correctness never depends on freshness: every posting flow re-reads the
/// authoritative `(id, MAX(sequence))` for the codes it used in its fence
/// statement and re-prepares on a mismatch. The cache only removes the round
/// trip that lookup would otherwise have cost on its own.
#[derive(Clone, Default)]
pub(super) struct TemplateCache {
    inner: Arc<RwLock<HashMap<String, ResolvedTemplate>>>,
}

impl TemplateCache {
    pub(super) fn get(&self, code: &str) -> Option<ResolvedTemplate> {
        self.inner
            .read()
            .expect("template cache poisoned")
            .get(code)
            .cloned()
    }

    pub(super) fn insert(&self, code: &str, resolved: ResolvedTemplate) {
        self.inner
            .write()
            .expect("template cache poisoned")
            .insert(code.to_string(), resolved);
    }

    /// Compare the versions this flow prepared against with the versions the
    /// fence statement observed. Returns the codes that must be re-resolved.
    ///
    /// A code missing from `observed` means the template was deleted between
    /// preparation and the fence; it is reported as stale so the re-resolution
    /// path surfaces the proper `NotFound`.
    pub(super) fn check(
        used: &HashMap<String, ResolvedTemplate>,
        observed: &HashMap<String, (TxTemplateId, i32)>,
    ) -> Vec<String> {
        used.iter()
            .filter(|(code, resolved)| {
                observed.get(*code) != Some(&(resolved.id, resolved.version))
            })
            .map(|(code, _)| code.clone())
            .collect()
    }
}

/// A posting evaluated down to the rows it will write.
///
/// `created_at` is deliberately absent: it comes from the fence statement's
/// `now()` (the transaction timestamp), so the `NewTransaction` is only built
/// once the fence has returned. Everything that determines the flow's lock set
/// — the entry accounts and currencies — is already fixed here.
pub(super) struct PreparedPosting {
    pub tx_id: TransactionId,
    pub journal_id: JournalId,
    pub tx_template_id: TxTemplateId,
    pub effective: NaiveDate,
    pub correlation_id: Option<String>,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub entries: Vec<NewEntry>,
}

impl PreparedPosting {
    pub(super) fn into_new_transaction(
        self,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> (NewTransaction, Vec<NewEntry>) {
        let mut builder = NewTransaction::builder();
        builder
            .id(self.tx_id)
            .created_at(created_at)
            .journal_id(self.journal_id)
            .tx_template_id(self.tx_template_id)
            .effective(self.effective)
            .entry_ids(self.entries.iter().map(|e| e.id).collect());
        if let Some(correlation_id) = self.correlation_id {
            builder.correlation_id(correlation_id);
        }
        if let Some(external_id) = self.external_id {
            builder.external_id(external_id);
        }
        if let Some(description) = self.description {
            builder.description(description);
        }
        if let Some(metadata) = self.metadata {
            builder.metadata(Some(metadata));
        }
        (
            builder.build().expect("tx_build should succeed"),
            self.entries,
        )
    }
}

/// Evaluate one posting's template against its params.
///
/// Pure: no clock read that matters to persistence, no database access. The
/// `clock` only seeds the CEL context (`date()`/`now()` builtins available to
/// template expressions), exactly as the pre-consolidation path did.
pub(super) fn prepare(
    clock: &es_entity::clock::ClockHandle,
    tmpl: &TxTemplateValues,
    tx_id: TransactionId,
    params: Params,
) -> Result<PreparedPosting, TxTemplateError> {
    let ctx = params.into_context(clock, tmpl.params.as_ref())?;

    let journal_id: Uuid = tmpl.transaction.journal_id.try_evaluate(&ctx)?;
    let journal_id = JournalId::from(journal_id);
    let entries = prep_entries(tmpl, tx_id, journal_id, &ctx)?;
    let effective: NaiveDate = tmpl.transaction.effective.try_evaluate(&ctx)?;

    let correlation_id = tmpl
        .transaction
        .correlation_id
        .as_ref()
        .map(|e| e.try_evaluate(&ctx))
        .transpose()?;
    let external_id = tmpl
        .transaction
        .external_id
        .as_ref()
        .map(|e| e.try_evaluate(&ctx))
        .transpose()?;
    let description = tmpl
        .transaction
        .description
        .as_ref()
        .map(|e| e.try_evaluate(&ctx))
        .transpose()?;
    let metadata = tmpl
        .transaction
        .metadata
        .as_ref()
        .map(|e| e.try_evaluate(&ctx))
        .transpose()?;

    Ok(PreparedPosting {
        tx_id,
        journal_id,
        tx_template_id: tmpl.id,
        effective,
        correlation_id,
        external_id,
        description,
        metadata,
        entries,
    })
}

fn prep_entries(
    tmpl: &TxTemplateValues,
    transaction_id: TransactionId,
    journal_id: JournalId,
    ctx: &cel_interpreter::CelContext,
) -> Result<Vec<NewEntry>, TxTemplateError> {
    let mut new_entries = Vec::with_capacity(tmpl.entries.len());
    let mut totals = HashMap::new();
    for (zero_based_sequence, entry) in tmpl.entries.iter().enumerate() {
        let mut builder = NewEntry::builder();
        builder
            .id(EntryId::new())
            .transaction_id(transaction_id)
            .journal_id(journal_id)
            .sequence(zero_based_sequence as u32 + 1);
        let account_id: Uuid = entry.account_id.try_evaluate(ctx)?;
        builder.account_id(account_id);

        let entry_type: String = entry.entry_type.try_evaluate(ctx)?;
        builder.entry_type(entry_type);

        let layer: Layer = entry.layer.try_evaluate(ctx)?;
        builder.layer(layer);

        let units: Decimal = entry.units.try_evaluate(ctx)?;
        let currency: Currency = entry.currency.try_evaluate(ctx)?;
        let direction: DebitOrCredit = entry.direction.try_evaluate(ctx)?;

        let total = totals.entry((currency, layer)).or_insert(Decimal::ZERO);
        match direction {
            DebitOrCredit::Debit => *total -= units,
            DebitOrCredit::Credit => *total += units,
        };
        builder.units(units);
        builder.currency(currency);
        builder.direction(direction);

        if let Some(description) = entry.description.as_ref() {
            let description: String = description.try_evaluate(ctx)?;
            builder.description(description);
        }

        if let Some(metadata) = entry.metadata.as_ref() {
            let metadata: serde_json::Value = metadata.try_evaluate(ctx)?;
            builder.metadata(metadata);
        }

        new_entries.push(builder.build().expect("Couldn't build entry"));
    }

    for ((c, l), v) in totals {
        if v != Decimal::ZERO {
            return Err(TxTemplateError::UnbalancedTransaction(c, l, v));
        }
    }

    Ok(new_entries)
}
