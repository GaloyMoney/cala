#![no_main]

use cala_ledger::{
    ec_rollup::{EcRollupBatch, PendingTx},
    entry::Entry,
};
use cala_types::{
    entry::EntryValues,
    primitives::{EntryId, JournalId, TransactionId},
};
use chrono::{DateTime, NaiveDate, Utc};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;
use std::collections::HashMap;

// `EcRollupBatch` is the pure accumulator behind the streaming EC-balance
// rollup: it collects transactions + entries, reports which entry ids a flush
// must still load, and assembles the applier input sorted by entry sequence.
// Input is two JSON documents (txns, entries) joined by a 0xFF separator.
fuzz_target!(|data: &[u8]| {
    let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
    if parts.len() < 2 {
        return;
    }
    let Ok(txs) = serde_json::from_slice::<Vec<FuzzTx>>(parts[0]) else {
        return;
    };
    let Ok(entries) = serde_json::from_slice::<Vec<EntryValues>>(parts[1]) else {
        return;
    };

    let mut batch = EcRollupBatch::default();
    for t in &txs {
        batch.push_tx(PendingTx {
            id: t.id,
            journal_id: t.journal_id,
            effective: t.effective,
            created_at: t.created_at,
            entry_ids: t.entry_ids.clone(),
        });
    }
    for e in &entries {
        batch.push_entry(e.clone());
    }

    let _missing = batch.missing_entry_ids();
    let _rollup = batch.into_rollup_txns(HashMap::<EntryId, Entry>::new());
});

#[derive(Deserialize)]
struct FuzzTx {
    id: TransactionId,
    journal_id: JournalId,
    effective: NaiveDate,
    created_at: DateTime<Utc>,
    entry_ids: Vec<EntryId>,
}
