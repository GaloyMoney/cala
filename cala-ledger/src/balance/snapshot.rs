use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot},
    entry::*,
    primitives::{DebitOrCredit, Layer},
};

use crate::primitives::{AccountId, EntryId};

use super::error::BalanceError;

pub(super) const UNASSIGNED_ENTRY_ID: uuid::Uuid = uuid::Uuid::nil();

pub(crate) struct Snapshots;

impl Snapshots {
    pub(crate) fn new_snapshot(
        time: DateTime<Utc>,
        account_id: AccountId,
        entry: &EntryValues,
    ) -> Result<BalanceSnapshot, BalanceError> {
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
    ) -> Result<BalanceSnapshot, BalanceError> {
        snapshot.version += 1;
        snapshot.modified_at = time;
        snapshot.entry_id = entry.id;
        // Decimal addition panics on overflow; a balance that exceeds
        // the representable range must roll the transaction back with
        // an error instead of crashing the caller's task.
        let account_id = snapshot.account_id;
        let add = |balance: &mut Decimal| {
            *balance = balance
                .checked_add(entry.units)
                .ok_or(BalanceError::Overflow(account_id))?;
            Ok::<(), BalanceError>(())
        };
        match entry.layer {
            Layer::Settled => {
                snapshot.settled.entry_id = entry.id;
                snapshot.settled.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        add(&mut snapshot.settled.dr_balance)?;
                    }
                    DebitOrCredit::Credit => {
                        add(&mut snapshot.settled.cr_balance)?;
                    }
                }
            }
            Layer::Pending => {
                snapshot.pending.entry_id = entry.id;
                snapshot.pending.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        add(&mut snapshot.pending.dr_balance)?;
                    }
                    DebitOrCredit::Credit => {
                        add(&mut snapshot.pending.cr_balance)?;
                    }
                }
            }
            Layer::Encumbrance => {
                snapshot.encumbrance.entry_id = entry.id;
                snapshot.encumbrance.modified_at = time;
                match entry.direction {
                    DebitOrCredit::Debit => {
                        add(&mut snapshot.encumbrance.dr_balance)?;
                    }
                    DebitOrCredit::Credit => {
                        add(&mut snapshot.encumbrance.cr_balance)?;
                    }
                }
            }
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cala_types::primitives::{JournalId, TransactionId};

    fn test_entry(units: Decimal) -> EntryValues {
        EntryValues {
            id: EntryId::new(),
            version: 1,
            transaction_id: TransactionId::new(),
            journal_id: JournalId::new(),
            account_id: AccountId::new(),
            entry_type: "TEST".to_string(),
            sequence: 1,
            layer: Layer::Settled,
            currency: "USD".parse().unwrap(),
            direction: DebitOrCredit::Debit,
            units,
            description: None,
            metadata: None,
        }
    }

    #[test]
    fn update_snapshot_errors_on_decimal_overflow_instead_of_panicking() {
        let entry = test_entry(Decimal::MAX);
        let snapshot = Snapshots::new_snapshot(Utc::now(), entry.account_id, &entry).unwrap();
        assert_eq!(snapshot.settled.dr_balance, Decimal::MAX);

        // Adding one more unit overflows Decimal's range
        let err = Snapshots::update_snapshot(Utc::now(), snapshot, &test_entry(Decimal::ONE))
            .expect_err("overflow must be an error, not a panic");
        assert!(matches!(err, BalanceError::Overflow(_)));
    }
}
