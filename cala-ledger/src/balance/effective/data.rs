use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use cala_types::{balance::BalanceSnapshot, entry::EntryValues};

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub(super) enum SnapshotOrEntry<'a> {
    Snapshot {
        effective: NaiveDate,
        values: BalanceSnapshot,
    },
    #[serde(skip_deserializing)]
    Entry {
        effective: NaiveDate,
        entry: &'a EntryValues,
    },
}

#[allow(dead_code)]
pub(super) struct EffectiveBalanceData<'a> {
    last_snapshot: Option<BalanceSnapshot>,
    updates: Vec<SnapshotOrEntry<'a>>,
}

impl<'a> EffectiveBalanceData<'a> {
    pub fn new(last_snapshot: Option<BalanceSnapshot>, updates: Vec<SnapshotOrEntry<'a>>) -> Self {
        Self {
            last_snapshot,
            updates,
        }
    }

    pub fn insert_entries(
        &mut self,
        effective: NaiveDate,
        entries: impl Iterator<Item = &'a EntryValues>,
    ) {
        for entry in entries {
            self.updates
                .push(SnapshotOrEntry::Entry { effective, entry });
        }
        // self.updates.sort();
    }
}

// add entries to vec as SnapshotOrEntry
// sort the vec
// update the vec in place Snapshots -> new values, Entries -> Snapshots
// persist
