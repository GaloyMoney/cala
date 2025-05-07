use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    entry::*,
    primitives::{DebitOrCredit, Layer},
};

use crate::primitives::{AccountId, EntryId};

pub(super) const UNASSIGNED_ENTRY_ID: uuid::Uuid = uuid::Uuid::nil();

pub(crate) struct Snapshots;

impl Snapshots {
    pub(crate) fn new_snapshot(
        time: DateTime<Utc>,
        account_id: AccountId,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        let entry_id = EntryId::from(UNASSIGNED_ENTRY_ID);
        Self::update_snapshot(
            time,
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
            },
            entry,
        )
    }

    pub(crate) fn update_snapshot(
        time: DateTime<Utc>,
        mut snapshot: BalanceSnapshot,
        entry: &EntryValues,
    ) -> BalanceSnapshot {
        snapshot.version += 1;
        snapshot.modified_at = time;
        snapshot.entry_id = entry.id;
        match entry.layer {
            Layer::Settled => {
                snapshot.settled.entry_id = entry.id;
                snapshot.settled.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.settled.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.settled.cr_balance += entry.units;
                    }
                }
            }
            Layer::Pending => {
                snapshot.pending.entry_id = entry.id;
                snapshot.pending.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.pending.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.pending.cr_balance += entry.units;
                    }
                }
            }
            Layer::Encumbrance => {
                snapshot.encumbrance.entry_id = entry.id;
                snapshot.encumbrance.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        snapshot.encumbrance.dr_balance += entry.units;
                    }
                    DebitOrCredit::Credit => {
                        snapshot.encumbrance.cr_balance += entry.units;
                    }
                }
            }
        }
        snapshot
    }
}
