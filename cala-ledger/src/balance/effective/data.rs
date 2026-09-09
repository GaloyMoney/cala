use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use std::cmp::Ordering;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot, EffectiveBalanceSnapshot},
    entry::EntryValues,
    primitives::{AccountId, Currency, EntryId, JournalId},
};

use crate::balance::snapshot::Snapshots;

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
        /// Position of the entry's transaction in the batch (landing order).
        /// Ties entries from the same transaction together and orders
        /// transactions relative to each other when several land on the
        /// same effective date; entries within a transaction still tie-break
        /// on `entry.sequence`.
        tx_index: usize,
        /// The transaction's `created_at`; becomes the new snapshot's
        /// `created_at`/`modified_at` when this entry is folded.
        created_at: DateTime<Utc>,
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

    fn snapshot(&self) -> (BalanceSnapshot, NaiveDate) {
        match self {
            Self::Snapshot { values, effective } => (values.clone(), *effective),
            _ => unimplemented!(),
        }
    }

    fn entry(&self) -> (&EntryValues, NaiveDate, DateTime<Utc>) {
        match self {
            Self::Entry {
                entry,
                effective,
                created_at,
                ..
            } => (entry, *effective, *created_at),
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub(super) struct EffectiveBalanceData<'a> {
    account_id: AccountId,
    currency: Currency,
    last_snapshot: Option<(NaiveDate, BalanceSnapshot)>,
    latest_all_time_version: u32,
    updates: Vec<SnapshotOrEntry<'a>>,
}

impl<'a> EffectiveBalanceData<'a> {
    pub fn new(
        account_id: AccountId,
        currency: Currency,
        last_snapshot: Option<(NaiveDate, BalanceSnapshot)>,
        latest_all_time_version: u32,
        updates: Vec<SnapshotOrEntry<'a>>,
    ) -> Self {
        Self {
            account_id,
            currency,
            last_snapshot,
            latest_all_time_version,
            updates,
        }
    }

    pub fn into_snapshots(
        self,
        journal_id: JournalId,
    ) -> impl Iterator<Item = EffectiveBalanceSnapshot> + use<'a> {
        self.updates
            .into_iter()
            .enumerate()
            .map(move |(idx, update)| {
                let (snapshot, effective) = update.snapshot();
                EffectiveBalanceSnapshot {
                    journal_id,
                    account_id: self.account_id,
                    currency: self.currency,
                    effective,
                    version: snapshot.version,
                    all_time_version: idx as u32 + 1 + self.latest_all_time_version,
                    created_at: snapshot.created_at,
                    modified_at: snapshot.modified_at,
                    entry_id: snapshot.entry_id,
                    settled: snapshot.settled,
                    pending: snapshot.pending,
                    encumbrance: snapshot.encumbrance,
                }
            })
    }

    pub fn push(
        &mut self,
        effective: NaiveDate,
        tx_index: usize,
        created_at: DateTime<Utc>,
        entry: &'a EntryValues,
    ) {
        self.updates.push(SnapshotOrEntry::Entry {
            effective,
            tx_index,
            created_at,
            entry,
        });
    }

    /// Replay `self.updates` (this pair's pre-existing later history plus
    /// whatever batch entries fanned into it) into a single ordered chain of
    /// snapshots. `rewritten_at` stamps `modified_at` on rows that already
    /// existed and are being rewritten; a new entry's own snapshot instead
    /// carries its transaction's `created_at` (see `SnapshotOrEntry::Entry`),
    /// so entries from different transactions in the same batch keep their
    /// own timestamps even though they're folded in one pass.
    pub fn re_calculate_snapshots(&mut self, rewritten_at: DateTime<Utc>) {
        // Nothing to recompute when there are no updates and no prior
        // snapshot to carry forward (the seeding path below indexes
        // `self.updates[0]`, which would otherwise panic).
        if self.updates.is_empty() {
            return;
        }
        self.updates.sort();
        let (mut last_balance, mut last_effective) = match self.last_snapshot.take() {
            Some((snapshot_date, snapshot)) => (snapshot, snapshot_date),
            None => {
                // Only legal when the earliest update is a batch entry: the
                // read is anchored at this pair's earliest effective date, so
                // every deleted row is strictly later than it, and the sort
                // places same-date entries after same-date snapshots.
                debug_assert!(
                    matches!(self.updates[0], SnapshotOrEntry::Entry { .. }),
                    "seeding without a prior snapshot requires the earliest \
                     update to be a batch entry, not a pre-existing row",
                );
                let (entry, effective, created_at) = self.updates[0].entry();
                (
                    Self::first_snapshot(created_at, self.account_id, entry),
                    effective,
                )
            }
        };

        let mut diff_snapshot = None;

        for update in self.updates.iter_mut() {
            if &last_effective != update.effective() {
                last_balance.version = 0;
            }
            match update {
                SnapshotOrEntry::Entry {
                    effective,
                    created_at,
                    entry,
                    ..
                } => {
                    let created_at = *created_at;
                    last_effective = *effective;
                    last_balance = Snapshots::update_snapshot(created_at, last_balance, entry);
                    diff_snapshot = if let Some(diff) = diff_snapshot {
                        Some(Snapshots::update_snapshot(created_at, diff, entry))
                    } else {
                        let mut initial = Self::first_snapshot(created_at, self.account_id, entry);
                        initial.entry_id = last_balance.entry_id;
                        initial.encumbrance.entry_id = last_balance.encumbrance.entry_id;
                        initial.pending.entry_id = last_balance.pending.entry_id;
                        initial.settled.entry_id = last_balance.settled.entry_id;
                        Some(Snapshots::update_snapshot(created_at, initial, entry))
                    };
                    *update = SnapshotOrEntry::Snapshot {
                        effective: *effective,
                        values: last_balance.clone(),
                    };
                }
                SnapshotOrEntry::Snapshot {
                    effective,
                    ref mut values,
                } => {
                    last_effective = *effective;
                    let diff = diff_snapshot.as_mut().expect("diff must be initialized");
                    values.modified_at = rewritten_at;
                    if diff.encumbrance.cr_balance != Decimal::ZERO
                        || diff.encumbrance.dr_balance != Decimal::ZERO
                    {
                        values.encumbrance.cr_balance += diff.encumbrance.cr_balance;
                        values.encumbrance.dr_balance += diff.encumbrance.dr_balance;
                        values.encumbrance.modified_at = rewritten_at;
                    }
                    if diff.pending.cr_balance != Decimal::ZERO
                        || diff.pending.dr_balance != Decimal::ZERO
                    {
                        values.pending.cr_balance += diff.pending.cr_balance;
                        values.pending.dr_balance += diff.pending.dr_balance;
                        values.pending.modified_at = rewritten_at;
                    }
                    if diff.settled.cr_balance != Decimal::ZERO
                        || diff.settled.dr_balance != Decimal::ZERO
                    {
                        values.settled.cr_balance += diff.settled.cr_balance;
                        values.settled.dr_balance += diff.settled.dr_balance;
                        values.settled.modified_at = rewritten_at;
                    }
                    if values.entry_id == values.encumbrance.entry_id {
                        diff.encumbrance.entry_id = values.entry_id;
                        values.pending.entry_id = diff.pending.entry_id;
                        values.settled.entry_id = diff.settled.entry_id;
                    }
                    if values.entry_id == values.pending.entry_id {
                        values.encumbrance.entry_id = diff.encumbrance.entry_id;
                        diff.pending.entry_id = values.entry_id;
                        values.settled.entry_id = diff.settled.entry_id;
                    }
                    if values.entry_id == values.settled.entry_id {
                        values.encumbrance.entry_id = diff.encumbrance.entry_id;
                        values.pending.entry_id = diff.pending.entry_id;
                        diff.settled.entry_id = values.entry_id;
                    }
                    // `last_balance` is the running "cumulative balance as
                    // of where the walk has reached" — the baseline the
                    // *next* `Entry` chains onto. The old per-transaction
                    // path never needed this arm to feed it back: a single
                    // transaction's entries always sorted strictly before
                    // every rewritten row, so nothing ever followed a
                    // rewrite. Batched across transactions, a later entry
                    // can now land after one or more rewritten rows (same
                    // date or a later one), so this rewritten row's own
                    // post-diff values — cumulative amounts *and*
                    // version — become the new baseline, exactly like an
                    // `Entry`-produced row would.
                    last_balance = values.clone();
                }
            }
        }
    }

    fn first_snapshot(
        time: DateTime<Utc>,
        account_id: AccountId,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        let entry_id = EntryId::from(crate::balance::snapshot::UNASSIGNED_ENTRY_ID);
        BalanceSnapshot {
            journal_id: entry.journal_id,
            account_id,
            entry_id,
            currency: entry.currency,
            settled: BalanceAmount {
                dr_balance: Decimal::ZERO,
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            pending: BalanceAmount {
                dr_balance: Decimal::ZERO,
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            encumbrance: BalanceAmount {
                dr_balance: Decimal::ZERO,
                cr_balance: Decimal::ZERO,
                entry_id,
                modified_at: time,
            },
            version: 0,
            modified_at: time,
            created_at: time,
        }
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
    // Applying the batch's transactions one at a time (the old per-tx path)
    // would, per pair and per date, hit the rows that already existed first
    // and then the batch's entries in posting order. This tie-break
    // reproduces exactly that sequence in one sort: a pre-existing row
    // (already in the table, deleted by the read) sorts before any batch
    // entry sharing its date, and batch entries tie-break on landing order
    // (`tx_index`) then intra-transaction order (`entry.sequence`).
    fn cmp(&self, other: &Self) -> Ordering {
        self.effective()
            .cmp(other.effective())
            .then_with(|| match (self, other) {
                (Self::Snapshot { .. }, Self::Entry { .. }) => Ordering::Less,
                (Self::Entry { .. }, Self::Snapshot { .. }) => Ordering::Greater,
                (Self::Snapshot { values: v1, .. }, Self::Snapshot { values: v2, .. }) => {
                    v1.version.cmp(&v2.version)
                }
                (
                    Self::Entry {
                        tx_index: t1,
                        entry: e1,
                        ..
                    },
                    Self::Entry {
                        tx_index: t2,
                        entry: e2,
                        ..
                    },
                ) => t1.cmp(t2).then(e1.sequence.cmp(&e2.sequence)),
            })
    }
}

#[cfg(test)]
mod tests {
    use cala_types::primitives::*;
    use rust_decimal_macros::dec;

    use super::*;

    fn entry_values() -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            journal_id: JournalId::new(),
            transaction_id: TransactionId::new(),
            account_id: AccountId::new(),
            currency: Currency::USD,
            entry_type: "ENTRY_TYPE".to_string(),
            sequence: 1,
            version: 1,
            layer: Layer::Settled,
            units: Decimal::ONE,
            direction: DebitOrCredit::Credit,
            description: None,
            metadata: None,
        }
    }

    fn balance_amount(entry_id: EntryId, credit: Decimal) -> BalanceAmount {
        BalanceAmount {
            dr_balance: Decimal::ZERO,
            cr_balance: credit,
            entry_id,
            modified_at: Utc::now(),
        }
    }

    fn random_snapshot() -> BalanceSnapshot {
        let entry_id = EntryId::new();
        BalanceSnapshot {
            journal_id: JournalId::new(),
            account_id: AccountId::new(),
            currency: Currency::USD,
            version: 1,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            entry_id,
            settled: balance_amount(entry_id, Decimal::ONE),
            pending: balance_amount(EntryId::new(), Decimal::ZERO),
            encumbrance: balance_amount(EntryId::new(), Decimal::ZERO),
        }
    }

    fn random_snapshot_with_pending() -> BalanceSnapshot {
        let entry_id = EntryId::new();
        BalanceSnapshot {
            journal_id: JournalId::new(),
            account_id: AccountId::new(),
            currency: Currency::USD,
            version: 1,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            entry_id,
            settled: balance_amount(EntryId::new(), Decimal::ONE),
            pending: balance_amount(entry_id, Decimal::ONE),
            encumbrance: balance_amount(EntryId::new(), Decimal::ZERO),
        }
    }

    #[test]
    fn empty_data() {
        let account_id = AccountId::new();
        let mut data = EffectiveBalanceData::new(account_id, Currency::USD, None, 0, Vec::new());

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        let posted_at = Utc::now();
        data.push(effective, 0, posted_at, &entry);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 1);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.settled.cr_balance, Decimal::ONE);
    }

    #[test]
    fn into_snapshots_stamps_journal_and_all_time_version() {
        let account_id = AccountId::new();
        let latest_all_time_version = 5;
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            None,
            latest_all_time_version,
            Vec::new(),
        );

        let day_one = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let day_two = NaiveDate::from_ymd_opt(2023, 10, 2).unwrap();
        let entry_one = entry_values();
        let entry_two = entry_values();
        let posted_at = Utc::now();
        data.push(day_one, 0, posted_at, &entry_one);
        data.push(day_two, 0, posted_at, &entry_two);
        data.re_calculate_snapshots(posted_at);

        let journal_id = JournalId::new();
        let snapshots: Vec<EffectiveBalanceSnapshot> = data.into_snapshots(journal_id).collect();

        assert_eq!(snapshots.len(), 2);
        assert!(snapshots.iter().all(|s| s.journal_id == journal_id));
        assert!(snapshots.iter().all(|s| s.account_id == account_id));
        assert!(snapshots.iter().all(|s| s.currency == Currency::USD));
        // all_time_version = enumerate idx + 1 + latest_all_time_version
        assert_eq!(snapshots[0].all_time_version, latest_all_time_version + 1);
        assert_eq!(snapshots[1].all_time_version, latest_all_time_version + 2);
        assert_eq!(snapshots[0].effective, day_one);
        assert_eq!(snapshots[1].effective, day_two);
    }

    #[test]
    fn existing_previous_balance() {
        let account_id = AccountId::new();
        let snapshot_date = NaiveDate::from_ymd_opt(2023, 9, 30).unwrap();
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            Some((snapshot_date, random_snapshot())),
            1,
            Vec::new(),
        );

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        let posted_at = Utc::now();
        data.push(effective, 0, posted_at, &entry);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 1);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.settled.cr_balance, dec!(2));
    }

    #[test]
    fn existing_previous_balance_same_day() {
        let account_id = AccountId::new();
        let snapshot_date = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            Some((snapshot_date, random_snapshot())),
            1,
            Vec::new(),
        );

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        let posted_at = Utc::now();
        data.push(effective, 0, posted_at, &entry);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 1);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.settled.cr_balance, dec!(2));
    }

    #[test]
    fn two_entries() {
        let account_id = AccountId::new();
        let mut data = EffectiveBalanceData::new(account_id, Currency::USD, None, 0, Vec::new());

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        let posted_at = Utc::now();
        data.push(effective, 0, posted_at, &entry);
        let mut entry_two = entry_values();
        entry_two.sequence = 2;
        data.push(effective, 0, posted_at, &entry_two);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 2);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.settled.cr_balance, dec!(1));

        assert!(matches!(data.updates[1], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[1].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry_two.id);
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.settled.cr_balance, dec!(2));
    }

    #[test]
    fn rewrite_future_snapshot_after_two_entries() {
        let account_id = AccountId::new();
        let future = NaiveDate::from_ymd_opt(2023, 10, 2).unwrap();
        let future_balance = random_snapshot_with_pending();
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            None,
            0,
            vec![SnapshotOrEntry::Snapshot {
                effective: future,
                values: future_balance.clone(),
            }],
        );
        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        let posted_at = Utc::now();
        data.push(effective, 0, posted_at, &entry);
        let mut entry_two = entry_values();
        entry_two.sequence = 2;
        data.push(effective, 0, posted_at, &entry_two);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 3);

        let (snapshot, update_effective) = data.updates[2].snapshot();
        assert_eq!(update_effective, future);
        assert_eq!(snapshot.entry_id, future_balance.entry_id);
        assert_eq!(snapshot.version, 1);

        assert_eq!(snapshot.settled.cr_balance, dec!(3));
        assert_eq!(snapshot.settled.entry_id, entry_two.id);
        assert_eq!(snapshot.pending.cr_balance, dec!(1));
        assert_eq!(snapshot.entry_id, snapshot.pending.entry_id);
    }

    /// A pair's pre-existing later history and the batch's new entries must
    /// interleave with the old rows first: deleted snapshots at D+1
    /// (versions 1..3) come before a same-date entry from the batch, and an
    /// earlier-dated batch entry seeds the walk before any of them.
    #[test]
    fn sort_places_existing_rows_before_new_entries_on_the_same_date() {
        let account_id = AccountId::new();
        let day = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let next_day = NaiveDate::from_ymd_opt(2023, 10, 2).unwrap();

        let mut old_versions = Vec::new();
        for v in 1..=3u32 {
            let mut snap = random_snapshot();
            snap.version = v;
            old_versions.push(SnapshotOrEntry::Snapshot {
                effective: next_day,
                values: snap,
            });
        }

        let mut data = EffectiveBalanceData::new(account_id, Currency::USD, None, 0, old_versions);

        let entry_on_day = entry_values();
        let entry_on_next_day = entry_values();
        let posted_at = Utc::now();
        // tx 0 at `day` seeds the walk; tx 1 at `next_day` must land after
        // the three pre-existing rows at that date, not before them.
        data.push(day, 0, posted_at, &entry_on_day);
        data.push(next_day, 1, posted_at, &entry_on_next_day);

        data.re_calculate_snapshots(posted_at);

        assert_eq!(data.updates.len(), 5);
        let (_, effective0) = data.updates[0].snapshot();
        assert_eq!(effective0, day, "the seeding entry comes first");
        for (idx, expected_version) in (1..=3u32).enumerate() {
            let (snapshot, effective) = data.updates[idx + 1].snapshot();
            assert_eq!(effective, next_day);
            assert_eq!(
                snapshot.version, expected_version,
                "pre-existing rows keep their relative order"
            );
        }
        let (last, effective_last) = data.updates[4].snapshot();
        assert_eq!(effective_last, next_day);
        assert_eq!(
            last.entry_id, entry_on_next_day.id,
            "the new entry lands after every pre-existing row on its date"
        );
        assert_eq!(
            last.settled.cr_balance,
            dec!(3),
            "cumulative must chain off the last rewritten row's shifted \
             total (1 orig + 1 diff = 2), plus this entry's own delta (+1)"
        );
        assert_eq!(
            last.version, 4,
            "must continue counting from the highest pre-existing version on \
             this date (3), not from whatever the entry chain last held",
        );
    }

    /// Two transactions with overlapping intra-transaction `sequence`
    /// numbers at the same date must keep posting order (`tx_index`), not
    /// interleave by `sequence` alone.
    #[test]
    fn entries_from_different_transactions_keep_posting_order() {
        let account_id = AccountId::new();
        let day = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();

        let mut tx0_entry1 = entry_values();
        tx0_entry1.sequence = 1;
        let mut tx0_entry2 = entry_values();
        tx0_entry2.sequence = 2;
        let mut tx1_entry1 = entry_values();
        tx1_entry1.sequence = 1;
        let mut tx1_entry2 = entry_values();
        tx1_entry2.sequence = 2;

        let mut data = EffectiveBalanceData::new(account_id, Currency::USD, None, 0, Vec::new());
        let posted_at = Utc::now();
        // Push tx 1's entries before tx 0's to prove the sort — not landing
        // order — is what fixes the final order.
        data.push(day, 1, posted_at, &tx1_entry1);
        data.push(day, 1, posted_at, &tx1_entry2);
        data.push(day, 0, posted_at, &tx0_entry1);
        data.push(day, 0, posted_at, &tx0_entry2);

        data.re_calculate_snapshots(posted_at);

        let ids: Vec<_> = data
            .updates
            .iter()
            .map(|u| u.snapshot().0.entry_id)
            .collect();
        assert_eq!(
            ids,
            vec![tx0_entry1.id, tx0_entry2.id, tx1_entry1.id, tx1_entry2.id],
            "tx_index must group each transaction's entries together, in landing order"
        );
    }

    fn to_balance_snapshot(s: &EffectiveBalanceSnapshot) -> BalanceSnapshot {
        BalanceSnapshot {
            journal_id: s.journal_id,
            account_id: s.account_id,
            entry_id: s.entry_id,
            currency: s.currency,
            settled: s.settled.clone(),
            pending: s.pending.clone(),
            encumbrance: s.encumbrance.clone(),
            version: s.version,
            modified_at: s.modified_at,
            created_at: s.created_at,
        }
    }

    /// The fold's single-pass replay must produce the same per-pair result
    /// as applying the batch's transactions one at a time (today's
    /// pre-batching behaviour), for a shuffled mix of dates and
    /// transactions — including "a later date's transaction arrives first,
    /// then a backdated one" — **and** a pre-existing future row that every
    /// one of the batch's dates falls before, so a single fold call must
    /// shift it forward by the *cumulative* diff of several transactions at
    /// once, not just the last one (the case a per-transaction round trip
    /// never has to handle, since it always sees exactly one entry's worth
    /// of diff at a time).
    #[test]
    fn fold_matches_sequential_application() {
        let account_id = AccountId::new();
        let d0 = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let d1 = NaiveDate::from_ymd_opt(2023, 10, 2).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2023, 10, 3).unwrap();
        let future = NaiveDate::from_ymd_opt(2023, 10, 4).unwrap();

        let seed = random_snapshot_with_pending();
        let pre_existing = EffectiveBalanceSnapshot {
            journal_id: seed.journal_id,
            account_id,
            currency: Currency::USD,
            effective: future,
            version: seed.version,
            all_time_version: 1,
            created_at: seed.created_at,
            modified_at: seed.modified_at,
            entry_id: seed.entry_id,
            settled: seed.settled,
            pending: seed.pending,
            encumbrance: seed.encumbrance,
        };

        // Landing order: D1 arrives first, then a backdated D0 (which must
        // shift the D1 row forward), then D2, then a second D1 entry. All
        // four dates precede the pre-existing `future` row.
        let tx_plan = [d1, d0, d2, d1];
        let entries: Vec<EntryValues> = tx_plan
            .iter()
            .map(|_| {
                let mut e = entry_values();
                e.sequence = 1;
                e
            })
            .collect();
        let posted_at = Utc::now();

        // Ground truth: apply one transaction at a time, each through its
        // own `EffectiveBalanceData` seeded exactly as `find_ec_for_update`
        // would seed it — `last_snapshot` is the table's highest
        // `all_time_version` row with `effective <=` this step's date,
        // `updates` starts as every row with `effective >` it (deleted and
        // about to be replayed, mirroring the real destructive read).
        let mut table: Vec<EffectiveBalanceSnapshot> = vec![pre_existing.clone()];
        for (tx_index, &effective) in tx_plan.iter().enumerate() {
            let last_snapshot = table
                .iter()
                .filter(|s| s.effective <= effective)
                .max_by_key(|s| s.all_time_version)
                .cloned();
            let (deleted, kept): (Vec<_>, Vec<_>) =
                table.into_iter().partition(|s| s.effective > effective);
            table = kept;

            let mut step = EffectiveBalanceData::new(
                account_id,
                Currency::USD,
                last_snapshot
                    .as_ref()
                    .map(|s| (s.effective, to_balance_snapshot(s))),
                last_snapshot.map(|s| s.all_time_version).unwrap_or(0),
                deleted
                    .iter()
                    .map(|s| SnapshotOrEntry::Snapshot {
                        effective: s.effective,
                        values: to_balance_snapshot(s),
                    })
                    .collect(),
            );
            step.push(effective, 0, posted_at, &entries[tx_index]);
            step.re_calculate_snapshots(posted_at);
            table.extend(step.into_snapshots(JournalId::new()));
        }
        table.sort_by_key(|s| s.all_time_version);

        // Single fold: everything pushed in landing order in one pass. The
        // pre-existing future row is seeded exactly as `find_ec_for_update`
        // would return it: no anchor (nothing exists at-or-before the
        // batch's earliest date), and the future row as the sole deleted
        // update — so the fold must shift it by all four entries' combined
        // diff in one pass, matching the four separate shifts above.
        let mut folded = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            None,
            0,
            vec![SnapshotOrEntry::Snapshot {
                effective: future,
                values: to_balance_snapshot(&pre_existing),
            }],
        );
        for (tx_index, &effective) in tx_plan.iter().enumerate() {
            folded.push(effective, tx_index, posted_at, &entries[tx_index]);
        }
        folded.re_calculate_snapshots(posted_at);
        let mut folded_snapshots: Vec<_> = folded.into_snapshots(JournalId::new()).collect();
        folded_snapshots.sort_by_key(|s| s.all_time_version);

        assert_eq!(folded_snapshots.len(), table.len());
        for (folded, sequential) in folded_snapshots.iter().zip(table.iter()) {
            assert_eq!(folded.effective, sequential.effective);
            assert_eq!(
                folded.version, sequential.version,
                "at {}",
                folded.effective
            );
            assert_eq!(
                folded.settled, sequential.settled,
                "settled at {}",
                folded.effective
            );
            assert_eq!(
                folded.pending, sequential.pending,
                "pending at {}",
                folded.effective
            );
            assert_eq!(
                folded.encumbrance, sequential.encumbrance,
                "encumbrance at {}",
                folded.effective
            );
        }
    }
}
