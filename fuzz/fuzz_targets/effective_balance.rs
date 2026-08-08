#![no_main]

use cala_ledger::balance::effective::{EffectiveBalanceData, SnapshotOrEntry};
use cala_types::{
    balance::BalanceSnapshot,
    entry::EntryValues,
    primitives::{AccountId, Currency, JournalId},
};
use chrono::NaiveDate;
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

// `EffectiveBalanceData::re_calculate_snapshots` recomputes date-partitioned
// balances from a list of entries and prior snapshots. It is pure and carries
// several unchecked invariants (no empty updates without a prior snapshot; no
// snapshot processed before an entry; no same-date entry+snapshot mix that the
// Ord impl can't compare). Fuzzing probes whether those hold.
//
// Input is four JSON documents concatenated with a 0xFF separator:
//   [entries] [dated-snapshots] [optional last snapshot] [plan]
fuzz_target!(|data: &[u8]| {
    let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
    if parts.len() < 4 {
        return;
    }
    let Ok(entries) = serde_json::from_slice::<Vec<EntryValues>>(parts[0]) else {
        return;
    };
    let Ok(snapshots) = serde_json::from_slice::<Vec<DatedSnapshot>>(parts[1]) else {
        return;
    };
    let last = serde_json::from_slice::<DatedSnapshot>(parts[2]).ok();
    let Ok(plan) = serde_json::from_slice::<Vec<PlanOp>>(parts[3]) else {
        return;
    };

    let account_id = AccountId::from(uuid::Uuid::nil());
    let currency = Currency::USD;
    let last = last.map(|d| (d.effective, d.values));

    // Build a mixed updates vec from the plan, interleaving entries and
    // snapshots so the sort/comparison logic sees both kinds.
    let mut updates: Vec<SnapshotOrEntry> = Vec::new();
    for op in &plan {
        match op.kind.as_str() {
            "entry" => {
                if let Some(entry) = entries.get(op.idx) {
                    updates.push(SnapshotOrEntry::Entry {
                        effective: op.effective,
                        entry,
                    });
                }
            }
            "snapshot" => {
                if let Some(snap) = snapshots.get(op.idx) {
                    updates.push(SnapshotOrEntry::Snapshot {
                        effective: op.effective,
                        values: snap.values.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    let mut data = EffectiveBalanceData::new(account_id, currency, last, 0, updates);
    data.re_calculate_snapshots(chrono::Utc::now());
    let _ = data.into_snapshots(JournalId::from(uuid::Uuid::nil())).count();
});

#[derive(Deserialize)]
struct DatedSnapshot {
    effective: NaiveDate,
    values: BalanceSnapshot,
}

#[derive(Deserialize)]
struct PlanOp {
    effective: NaiveDate,
    kind: String,
    idx: usize,
}
