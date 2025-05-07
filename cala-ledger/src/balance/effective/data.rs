use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use std::cmp::Ordering;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    entry::EntryValues,
    primitives::{AccountId, Currency, EntryId},
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

    fn entry(&self) -> (&EntryValues, NaiveDate) {
        match self {
            Self::Entry { entry, effective } => (entry, *effective),
            _ => unimplemented!(),
        }
    }
}

pub(super) struct EffectiveBalanceData<'a> {
    account_id: AccountId,
    currency: Currency,
    last_snapshot: Option<BalanceSnapshot>,
    latest_all_time_version: u32,
    updates: Vec<SnapshotOrEntry<'a>>,
}

impl<'a> EffectiveBalanceData<'a> {
    pub fn new(
        account_id: AccountId,
        currency: Currency,
        last_snapshot: Option<BalanceSnapshot>,
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

    pub fn into_updates(
        self,
    ) -> impl Iterator<Item = (AccountId, Currency, NaiveDate, BalanceSnapshot, u32)> + use<'a>
    {
        self.updates
            .into_iter()
            .enumerate()
            .map(move |(idx, update)| {
                let (values, effective) = update.snapshot();
                (
                    self.account_id,
                    self.currency,
                    effective,
                    values,
                    idx as u32 + 1 + self.latest_all_time_version,
                )
            })
    }

    pub fn push(&mut self, effective: NaiveDate, entry: &'a EntryValues) {
        self.updates
            .push(SnapshotOrEntry::Entry { effective, entry });
    }

    pub fn re_calculate_snapshots(&mut self, created_at: DateTime<Utc>, effective: NaiveDate) {
        self.updates.sort();
        let start_idx = self
            .updates
            .iter()
            .position(|item| matches!(item, SnapshotOrEntry::Entry { .. }));
        let ((mut last_balance, mut last_effective), start_idx) =
            match (start_idx, self.last_snapshot.take()) {
                (Some(idx), _) if idx > 0 => (self.updates[idx - 1].snapshot(), idx),
                (_, Some(snapshot)) => ((snapshot, effective - chrono::Days::new(1)), 0),
                (_, None) => {
                    let (entry, effective) = self.updates[0].entry();
                    (
                        (
                            Self::first_snapshot(created_at, self.account_id, entry),
                            effective,
                        ),
                        0,
                    )
                }
            };

        let mut diff_snapshot = None;

        for update in self.updates.iter_mut().skip(start_idx) {
            if &last_effective != update.effective() {
                last_balance.version = 0;
            }
            match update {
                SnapshotOrEntry::Entry { effective, entry } => {
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
                    values.modified_at = created_at;
                    if diff.encumbrance.cr_balance != Decimal::ZERO
                        || diff.encumbrance.dr_balance != Decimal::ZERO
                    {
                        values.encumbrance.cr_balance += diff.encumbrance.cr_balance;
                        values.encumbrance.dr_balance += diff.encumbrance.dr_balance;
                        values.encumbrance.modified_at = created_at;
                    }
                    if diff.pending.cr_balance != Decimal::ZERO
                        || diff.pending.dr_balance != Decimal::ZERO
                    {
                        values.pending.cr_balance += diff.pending.cr_balance;
                        values.pending.dr_balance += diff.pending.dr_balance;
                        values.pending.modified_at = created_at;
                    }
                    if diff.settled.cr_balance != Decimal::ZERO
                        || diff.settled.dr_balance != Decimal::ZERO
                    {
                        values.settled.cr_balance += diff.settled.cr_balance;
                        values.settled.dr_balance += diff.settled.dr_balance;
                        values.settled.modified_at = created_at;
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
        data.push(effective, &entry);

        let posted_at = Utc::now();
        data.re_calculate_snapshots(posted_at, effective);

        assert_eq!(data.updates.len(), 1);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.settled.cr_balance, Decimal::ONE);
    }

    #[test]
    fn existing_previous_balance() {
        let account_id = AccountId::new();
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            Some(random_snapshot()),
            1,
            Vec::new(),
        );

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        data.push(effective, &entry);

        let posted_at = Utc::now();
        data.re_calculate_snapshots(posted_at, effective);

        assert_eq!(data.updates.len(), 1);
        assert!(matches!(data.updates[0], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[0].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.settled.cr_balance, dec!(2));
    }

    #[test]
    fn two_entries() {
        let account_id = AccountId::new();
        let mut data = EffectiveBalanceData::new(account_id, Currency::USD, None, 0, Vec::new());

        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let entry = entry_values();
        data.push(effective, &entry);
        let mut entry_two = entry_values();
        entry_two.sequence = 2;
        data.push(effective, &entry_two);

        let posted_at = Utc::now();
        data.re_calculate_snapshots(posted_at, effective);

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
    fn previous_snapshot_same_day() {
        let account_id = AccountId::new();
        let effective = NaiveDate::from_ymd_opt(2023, 10, 1).unwrap();
        let mut data = EffectiveBalanceData::new(
            account_id,
            Currency::USD,
            None,
            0,
            vec![SnapshotOrEntry::Snapshot {
                effective,
                values: random_snapshot(),
            }],
        );
        let entry = entry_values();
        data.push(effective, &entry);

        let posted_at = Utc::now();
        data.re_calculate_snapshots(posted_at, effective);

        assert_eq!(data.updates.len(), 2);
        assert!(matches!(data.updates[1], SnapshotOrEntry::Snapshot { .. }));

        let (snapshot, update_effective) = data.updates[1].snapshot();
        assert_eq!(update_effective, effective);
        assert_eq!(snapshot.entry_id, entry.id);
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
        data.push(effective, &entry);
        let mut entry_two = entry_values();
        entry_two.sequence = 2;
        data.push(effective, &entry_two);

        let posted_at = Utc::now();
        data.re_calculate_snapshots(posted_at, effective);

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
}
