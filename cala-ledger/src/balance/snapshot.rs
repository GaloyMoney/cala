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

#[cfg(test)]
mod proptests {
    //! Property-based tests for `Snapshots::new_snapshot` and `update_snapshot`.
    //!
    //! Properties:
    //!   - Layer/direction dispatch: an entry only mutates the (layer, direction)
    //!     cell it targets. The other 5 of 6 cells are untouched.
    //!   - Version monotonicity: each `update_snapshot` increments version by 1.
    //!   - Sum-of-units: applying N entries to one (layer, direction) cell makes
    //!     that cell equal the sum of their units.
    //!   - Order independence: applying any permutation of the same multiset of
    //!     entries yields the same balance amounts (debits == debits, credits ==
    //!     credits regardless of order). Foundation of the double-entry contract.
    //!   - `BalanceSnapshot::available` rollup composition: settled is exactly
    //!     `available(Settled)`; pending rollup adds pending+settled; encumbrance
    //!     rollup adds all three.

    use proptest::prelude::*;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use cala_types::{
        balance::BalanceSnapshot,
        entry::EntryValues,
        primitives::{Currency, DebitOrCredit, JournalId, Layer, TransactionId},
    };

    use super::*;

    fn arb_layer() -> impl Strategy<Value = Layer> {
        prop_oneof![Just(Layer::Settled), Just(Layer::Pending), Just(Layer::Encumbrance)]
    }

    fn arb_direction() -> impl Strategy<Value = DebitOrCredit> {
        prop_oneof![Just(DebitOrCredit::Debit), Just(DebitOrCredit::Credit)]
    }

    /// Units in cents-ish range; positive only. Decimal::from_i64 covers up to
    /// ~9e18 which is well within rust_decimal's representable range when
    /// summed across a small number of entries.
    fn arb_units() -> impl Strategy<Value = Decimal> {
        (0u64..=1_000_000_000_000u64).prop_map(Decimal::from)
    }

    fn entry(layer: Layer, direction: DebitOrCredit, units: Decimal) -> EntryValues {
        EntryValues {
            id: EntryId::from(Uuid::nil()),
            version: 1,
            transaction_id: TransactionId::from(Uuid::nil()),
            journal_id: JournalId::from(Uuid::nil()),
            account_id: AccountId::from(Uuid::nil()),
            entry_type: "TEST".to_string(),
            sequence: 1,
            layer,
            units,
            currency: Currency::USD,
            direction,
            description: None,
            metadata: None,
        }
    }

    fn time(seconds: i64) -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
    }

    /// All six (layer, direction) cells in a snapshot, returned as `(layer, direction, value)`
    /// for compact comparison.
    fn cells(s: &BalanceSnapshot) -> Vec<(Layer, DebitOrCredit, Decimal)> {
        vec![
            (Layer::Settled, DebitOrCredit::Debit, s.settled.dr_balance),
            (Layer::Settled, DebitOrCredit::Credit, s.settled.cr_balance),
            (Layer::Pending, DebitOrCredit::Debit, s.pending.dr_balance),
            (Layer::Pending, DebitOrCredit::Credit, s.pending.cr_balance),
            (Layer::Encumbrance, DebitOrCredit::Debit, s.encumbrance.dr_balance),
            (Layer::Encumbrance, DebitOrCredit::Credit, s.encumbrance.cr_balance),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

        /// `new_snapshot` produces a snapshot with all-zero balance cells except
        /// the one cell (layer, direction) that the entry targets.
        #[test]
        fn new_snapshot_only_touches_target_cell(
            layer in arb_layer(),
            direction in arb_direction(),
            units in arb_units(),
        ) {
            let entry = entry(layer, direction, units);
            let snap = Snapshots::new_snapshot(time(0), entry.account_id, &entry);

            for (l, d, v) in cells(&snap) {
                if l == layer && d == direction {
                    prop_assert_eq!(v, units, "target cell should equal entry.units");
                } else {
                    prop_assert_eq!(v, Decimal::ZERO, "non-target cell ({:?}, {:?}) should be zero", l, d);
                }
            }
        }

        /// `update_snapshot` increments version by exactly 1 per call.
        #[test]
        fn version_increments_by_one(
            n in 1usize..30,
            layer in arb_layer(),
            direction in arb_direction(),
            units in arb_units(),
        ) {
            let initial_entry = entry(layer, direction, units);
            let mut snap = Snapshots::new_snapshot(time(0), initial_entry.account_id, &initial_entry);
            prop_assert_eq!(snap.version, 1);

            for i in 1..n {
                snap = Snapshots::update_snapshot(time(i as i64), snap, &initial_entry);
                prop_assert_eq!(snap.version, (i + 1) as u32);
            }
        }

        /// Sum-of-units: applying N debit entries to a single (layer) yields
        /// dr_balance == sum(units) on that layer. Cross-layer cells are zero.
        #[test]
        fn sum_of_units_lands_on_target_cell(
            layer in arb_layer(),
            direction in arb_direction(),
            unit_list in proptest::collection::vec(arb_units(), 1..15),
        ) {
            let account = AccountId::from(Uuid::nil());
            let mut snap: Option<BalanceSnapshot> = None;
            for (i, &u) in unit_list.iter().enumerate() {
                let mut e = entry(layer, direction, u);
                e.account_id = account;
                snap = Some(match snap {
                    None => Snapshots::new_snapshot(time(i as i64), account, &e),
                    Some(s) => Snapshots::update_snapshot(time(i as i64), s, &e),
                });
            }
            let snap = snap.unwrap();
            let expected_total: Decimal = unit_list.iter().sum();

            for (l, d, v) in cells(&snap) {
                if l == layer && d == direction {
                    prop_assert_eq!(v, expected_total);
                } else {
                    prop_assert_eq!(v, Decimal::ZERO);
                }
            }
        }

        /// Order independence: applying entries in any permutation yields the
        /// same final balance amounts. This is the additive-commutativity
        /// property that the entire double-entry contract rests on at the
        /// per-account level.
        #[test]
        fn balance_amounts_are_order_independent(
            entries_spec in proptest::collection::vec(
                (arb_layer(), arb_direction(), arb_units()),
                1..10,
            ),
            permutation_seed in any::<u64>(),
        ) {
            let account = AccountId::from(Uuid::nil());

            // Original order
            let mut snap_a: Option<BalanceSnapshot> = None;
            for (i, &(l, d, u)) in entries_spec.iter().enumerate() {
                let mut e = entry(l, d, u);
                e.account_id = account;
                snap_a = Some(match snap_a {
                    None => Snapshots::new_snapshot(time(i as i64), account, &e),
                    Some(s) => Snapshots::update_snapshot(time(i as i64), s, &e),
                });
            }

            // Shuffled order via deterministic seeded RNG
            use rand::seq::SliceRandom;
            use rand::SeedableRng;
            let mut shuffled = entries_spec.clone();
            let mut rng = rand::rngs::StdRng::seed_from_u64(permutation_seed);
            shuffled.shuffle(&mut rng);

            let mut snap_b: Option<BalanceSnapshot> = None;
            for (i, &(l, d, u)) in shuffled.iter().enumerate() {
                let mut e = entry(l, d, u);
                e.account_id = account;
                snap_b = Some(match snap_b {
                    None => Snapshots::new_snapshot(time(i as i64), account, &e),
                    Some(s) => Snapshots::update_snapshot(time(i as i64), s, &e),
                });
            }

            prop_assert_eq!(cells(&snap_a.unwrap()), cells(&snap_b.unwrap()));
        }

        /// `BalanceSnapshot::available` rollup composition.
        ///   - available(Settled) == settled
        ///   - available(Pending).dr - available(Settled).dr == pending.dr
        ///   - available(Encumbrance).dr - available(Pending).dr == encumbrance.dr
        /// Same for cr_balance.
        #[test]
        fn available_rollup_composition(
            entries_spec in proptest::collection::vec(
                (arb_layer(), arb_direction(), arb_units()),
                1..10,
            ),
        ) {
            let account = AccountId::from(Uuid::nil());
            let mut snap: Option<BalanceSnapshot> = None;
            for (i, &(l, d, u)) in entries_spec.iter().enumerate() {
                let mut e = entry(l, d, u);
                e.account_id = account;
                snap = Some(match snap {
                    None => Snapshots::new_snapshot(time(i as i64), account, &e),
                    Some(s) => Snapshots::update_snapshot(time(i as i64), s, &e),
                });
            }
            let s = snap.unwrap();

            let av_settled = s.available(Layer::Settled);
            prop_assert_eq!(av_settled.dr_balance, s.settled.dr_balance);
            prop_assert_eq!(av_settled.cr_balance, s.settled.cr_balance);

            let av_pending = s.available(Layer::Pending);
            prop_assert_eq!(av_pending.dr_balance, s.settled.dr_balance + s.pending.dr_balance);
            prop_assert_eq!(av_pending.cr_balance, s.settled.cr_balance + s.pending.cr_balance);

            let av_enc = s.available(Layer::Encumbrance);
            prop_assert_eq!(
                av_enc.dr_balance,
                s.settled.dr_balance + s.pending.dr_balance + s.encumbrance.dr_balance
            );
            prop_assert_eq!(
                av_enc.cr_balance,
                s.settled.cr_balance + s.pending.cr_balance + s.encumbrance.cr_balance
            );
        }
    }
}
