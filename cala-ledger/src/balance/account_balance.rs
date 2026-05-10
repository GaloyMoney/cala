use rust_decimal::Decimal;

use crate::primitives::*;
use cala_types::balance::*;

/// Representation of account's balance tracked in 3 distinct layers.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub balance_type: DebitOrCredit,
    pub details: BalanceSnapshot,
}

impl AccountBalance {
    pub(crate) fn new(balance_type: DebitOrCredit, details: BalanceSnapshot) -> Self {
        Self {
            balance_type,
            details,
        }
    }

    pub(super) fn derive_diff(mut self, since: &Self) -> Self {
        self.details.settled = BalanceAmount {
            dr_balance: self.details.settled.dr_balance - since.details.settled.dr_balance,
            cr_balance: self.details.settled.cr_balance - since.details.settled.cr_balance,
            ..self.details.settled
        };
        self.details.pending = BalanceAmount {
            dr_balance: self.details.pending.dr_balance - since.details.pending.dr_balance,
            cr_balance: self.details.pending.cr_balance - since.details.pending.cr_balance,
            ..self.details.pending
        };
        self.details.encumbrance = BalanceAmount {
            dr_balance: self.details.encumbrance.dr_balance - since.details.encumbrance.dr_balance,
            cr_balance: self.details.encumbrance.cr_balance - since.details.encumbrance.cr_balance,
            ..self.details.encumbrance
        };
        self
    }

    pub fn pending(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .pending()
    }

    pub fn settled(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .settled()
    }

    pub fn encumbrance(&self) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .encumbrance()
    }

    pub fn available(&self, layer: Layer) -> Decimal {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .available(layer)
    }
}

pub(crate) struct BalanceWithDirection<'a> {
    direction: DebitOrCredit,
    details: &'a BalanceSnapshot,
}

impl<'a> BalanceWithDirection<'a> {
    pub fn new(direction: DebitOrCredit, details: &'a BalanceSnapshot) -> Self {
        Self { direction, details }
    }

    pub fn pending(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.pending.cr_balance - self.details.pending.dr_balance
        } else {
            self.details.pending.dr_balance - self.details.pending.cr_balance
        }
    }

    pub fn settled(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.settled.cr_balance - self.details.settled.dr_balance
        } else {
            self.details.settled.dr_balance - self.details.settled.cr_balance
        }
    }

    pub fn encumbrance(&self) -> Decimal {
        if self.direction == DebitOrCredit::Credit {
            self.details.encumbrance.cr_balance - self.details.encumbrance.dr_balance
        } else {
            self.details.encumbrance.dr_balance - self.details.encumbrance.cr_balance
        }
    }

    pub fn available(&self, layer: Layer) -> Decimal {
        match layer {
            Layer::Settled => self.settled(),
            Layer::Pending => self.pending() + self.settled(),
            Layer::Encumbrance => self.encumbrance() + self.pending() + self.settled(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BalanceRange {
    pub open: AccountBalance,
    pub period: AccountBalance,
    pub close: AccountBalance,
}

impl BalanceRange {
    pub fn new(start: Option<AccountBalance>, end: AccountBalance, version_diff: u32) -> Self {
        match start {
            Some(start) => {
                let close = end.clone();
                let mut period = end.derive_diff(&start);
                period.details.version = version_diff;
                Self {
                    close,
                    period,
                    open: start,
                }
            }
            None => {
                use chrono::{TimeZone, Utc};
                let zero_time = Utc.timestamp_opt(0, 0).single().expect("0 timestamp");
                let zero_entry = EntryId::from(super::snapshot::UNASSIGNED_ENTRY_ID);
                let zero_amount = BalanceAmount {
                    dr_balance: Decimal::ZERO,
                    cr_balance: Decimal::ZERO,
                    entry_id: zero_entry,
                    modified_at: zero_time,
                };
                let mut range = end.clone();
                range.details.version = version_diff;
                Self {
                    period: range,
                    close: end.clone(),
                    open: AccountBalance {
                        balance_type: end.balance_type,
                        details: BalanceSnapshot {
                            version: 0,
                            created_at: zero_time,
                            modified_at: zero_time,
                            entry_id: zero_entry,
                            settled: zero_amount.clone(),
                            pending: zero_amount.clone(),
                            encumbrance: zero_amount,
                            ..end.details
                        },
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    //! Property-based tests for `BalanceWithDirection` signed-balance math
    //! and `AccountBalance::derive_diff`.
    //!
    //! Properties:
    //!   - Signed-balance direction: for a Credit account, `settled() == cr - dr`;
    //!     for a Debit account, `settled() == dr - cr`. Sign flips on direction.
    //!     Same for pending() and encumbrance().
    //!   - `available(Settled)` always equals `settled()`.
    //!   - `available(Pending) == pending() + settled()`.
    //!   - `available(Encumbrance) == encumbrance() + pending() + settled()`.
    //!   - `derive_diff(zero_baseline) == self` on all four amount fields.
    //!   - `derive_diff(self) == zero` on all four amount fields.

    use chrono::TimeZone;
    use proptest::prelude::*;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use cala_types::balance::{BalanceAmount, BalanceSnapshot};

    use super::*;

    fn arb_amount_pair() -> impl Strategy<Value = (Decimal, Decimal)> {
        let v = 0u64..=1_000_000_000_000u64;
        (v.clone(), v).prop_map(|(d, c)| (Decimal::from(d), Decimal::from(c)))
    }

    fn make_balance_amount(dr: Decimal, cr: Decimal) -> BalanceAmount {
        BalanceAmount {
            dr_balance: dr,
            cr_balance: cr,
            entry_id: EntryId::from(Uuid::nil()),
            modified_at: chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        }
    }

    fn make_snapshot(
        settled: (Decimal, Decimal),
        pending: (Decimal, Decimal),
        encumbrance: (Decimal, Decimal),
    ) -> BalanceSnapshot {
        let t = chrono::Utc.timestamp_opt(0, 0).single().unwrap();
        BalanceSnapshot {
            journal_id: cala_types::primitives::JournalId::from(Uuid::nil()),
            account_id: AccountId::from(Uuid::nil()),
            currency: cala_types::primitives::Currency::USD,
            version: 1,
            created_at: t,
            modified_at: t,
            entry_id: EntryId::from(Uuid::nil()),
            settled: make_balance_amount(settled.0, settled.1),
            pending: make_balance_amount(pending.0, pending.1),
            encumbrance: make_balance_amount(encumbrance.0, encumbrance.1),
        }
    }

    fn arb_direction() -> impl Strategy<Value = DebitOrCredit> {
        prop_oneof![Just(DebitOrCredit::Debit), Just(DebitOrCredit::Credit)]
    }

    fn arb_layer() -> impl Strategy<Value = Layer> {
        prop_oneof![
            Just(Layer::Settled),
            Just(Layer::Pending),
            Just(Layer::Encumbrance),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

        /// `settled()` flips sign with the account direction.
        /// Credit account: `cr - dr`; Debit account: `dr - cr`.
        #[test]
        fn settled_direction_dependent(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);

            let credit_view = BalanceWithDirection::new(DebitOrCredit::Credit, &snap);
            let debit_view = BalanceWithDirection::new(DebitOrCredit::Debit, &snap);

            prop_assert_eq!(credit_view.settled(), settled.1 - settled.0);
            prop_assert_eq!(debit_view.settled(), settled.0 - settled.1);

            // Sign flip: credit_view.settled() + debit_view.settled() == 0
            prop_assert_eq!(credit_view.settled() + debit_view.settled(), Decimal::ZERO);
        }

        /// `pending()` and `encumbrance()` follow the same direction-flip rule.
        #[test]
        fn pending_and_encumbrance_direction_dependent(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
            dir in arb_direction(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);
            let view = BalanceWithDirection::new(dir, &snap);

            let expected_pending = match dir {
                DebitOrCredit::Credit => pending.1 - pending.0,
                DebitOrCredit::Debit => pending.0 - pending.1,
            };
            let expected_enc = match dir {
                DebitOrCredit::Credit => encumbrance.1 - encumbrance.0,
                DebitOrCredit::Debit => encumbrance.0 - encumbrance.1,
            };
            prop_assert_eq!(view.pending(), expected_pending);
            prop_assert_eq!(view.encumbrance(), expected_enc);
        }

        /// `available(Settled) == settled()`.
        /// `available(Pending) == pending() + settled()`.
        /// `available(Encumbrance) == encumbrance() + pending() + settled()`.
        #[test]
        fn available_layered_composition(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
            dir in arb_direction(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);
            let view = BalanceWithDirection::new(dir, &snap);

            prop_assert_eq!(view.available(Layer::Settled), view.settled());
            prop_assert_eq!(view.available(Layer::Pending), view.pending() + view.settled());
            prop_assert_eq!(
                view.available(Layer::Encumbrance),
                view.encumbrance() + view.pending() + view.settled()
            );
        }

        /// `AccountBalance::available(layer)` agrees with `BalanceWithDirection::available`.
        #[test]
        fn account_balance_available_matches_view(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
            dir in arb_direction(),
            layer in arb_layer(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);
            let ab = AccountBalance::new(dir, snap.clone());
            let view = BalanceWithDirection::new(dir, &snap);
            prop_assert_eq!(ab.available(layer), view.available(layer));
        }

        /// `derive_diff(zero_baseline) == self` on the four amount fields.
        #[test]
        fn derive_diff_with_zero_is_identity(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
            dir in arb_direction(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);
            let zero = make_snapshot(
                (Decimal::ZERO, Decimal::ZERO),
                (Decimal::ZERO, Decimal::ZERO),
                (Decimal::ZERO, Decimal::ZERO),
            );

            let result = AccountBalance::new(dir, snap.clone())
                .derive_diff(&AccountBalance::new(dir, zero));

            prop_assert_eq!(result.details.settled.dr_balance, snap.settled.dr_balance);
            prop_assert_eq!(result.details.settled.cr_balance, snap.settled.cr_balance);
            prop_assert_eq!(result.details.pending.dr_balance, snap.pending.dr_balance);
            prop_assert_eq!(result.details.pending.cr_balance, snap.pending.cr_balance);
            prop_assert_eq!(
                result.details.encumbrance.dr_balance,
                snap.encumbrance.dr_balance
            );
            prop_assert_eq!(
                result.details.encumbrance.cr_balance,
                snap.encumbrance.cr_balance
            );
        }

        /// `derive_diff(self)` produces zero balances on all four amount fields.
        #[test]
        fn derive_diff_with_self_is_zero(
            settled in arb_amount_pair(),
            pending in arb_amount_pair(),
            encumbrance in arb_amount_pair(),
            dir in arb_direction(),
        ) {
            let snap = make_snapshot(settled, pending, encumbrance);
            let result = AccountBalance::new(dir, snap.clone())
                .derive_diff(&AccountBalance::new(dir, snap));

            prop_assert_eq!(result.details.settled.dr_balance, Decimal::ZERO);
            prop_assert_eq!(result.details.settled.cr_balance, Decimal::ZERO);
            prop_assert_eq!(result.details.pending.dr_balance, Decimal::ZERO);
            prop_assert_eq!(result.details.pending.cr_balance, Decimal::ZERO);
            prop_assert_eq!(result.details.encumbrance.dr_balance, Decimal::ZERO);
            prop_assert_eq!(result.details.encumbrance.cr_balance, Decimal::ZERO);
        }
    }
}
