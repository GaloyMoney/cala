use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use std::cmp::Ordering;

use cala_types::{balance::BalanceSnapshot, entry::EntryValues, primitives::AccountId};

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

impl SnapshotOrEntry<'_> {
    pub fn effective(&self) -> &NaiveDate {
        match self {
            Self::Snapshot { effective, .. } => effective,
            Self::Entry { effective, .. } => effective,
        }
    }
}

pub(super) struct EffectiveBalanceData<'a> {
    account_id: AccountId,
    last_snapshot: Option<BalanceSnapshot>,
    updates: Vec<SnapshotOrEntry<'a>>,
}

impl<'a> EffectiveBalanceData<'a> {
    pub fn new(
        account_id: AccountId,
        last_snapshot: Option<BalanceSnapshot>,
        updates: Vec<SnapshotOrEntry<'a>>,
    ) -> Self {
        Self {
            account_id,
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
        self.updates.sort();
    }

    pub fn re_calculate_snapshots(&mut self, created_at: DateTime<Utc>) {
        let start_idx = self
            .updates
            .iter()
            .position(|item| matches!(item, SnapshotOrEntry::Entry { .. }));
        // initialize first balance
        // depends on is updates[0] a snapshot or entry
        //
    }
}

impl PartialEq for SnapshotOrEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Snapshot { values: v1, .. }, Self::Snapshot { values: v2, .. }) => {
                v1.entry_id == v2.entry_id
            }
            (Self::Entry { entry: en1, .. }, Self::Entry { entry: en2, .. }) => en1.id == en2.id,
            _ => false,
        }
    }
}
impl Eq for SnapshotOrEntry<'_> {}

impl PartialOrd for SnapshotOrEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SnapshotOrEntry<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.effective().cmp(other.effective()) {
            Ordering::Equal => {}
            ordering => return ordering,
        }

        match (self, other) {
            (Self::Snapshot { .. }, Self::Entry { .. }) => Ordering::Less,
            (Self::Entry { .. }, Self::Snapshot { .. }) => Ordering::Greater,
            (Self::Snapshot { values: v1, .. }, Self::Snapshot { values: v2, .. }) => {
                v1.version.cmp(&v2.version)
            }
            (Self::Entry { entry: e1, .. }, Self::Entry { entry: e2, .. }) => {
                e1.sequence.cmp(&e2.sequence)
            }
        }
    }
}
