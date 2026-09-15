use rust_decimal::Decimal;

use crate::primitives::*;
use cala_types::balance::*;

use super::error::BalanceError;

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

    /// Period delta for each layer, per balance side: `self` minus `since`.
    /// Returns `Err(BalanceError::Overflow)` if any subtraction exceeds the
    /// representable `Decimal` range.
    pub(super) fn derive_diff(mut self, since: &Self) -> Result<Self, BalanceError> {
        let account_id = self.details.account_id;
        self.details.settled = BalanceAmount {
            dr_balance: checked_sub(
                self.details.settled.dr_balance,
                since.details.settled.dr_balance,
                account_id,
            )?,
            cr_balance: checked_sub(
                self.details.settled.cr_balance,
                since.details.settled.cr_balance,
                account_id,
            )?,
            ..self.details.settled
        };
        self.details.pending = BalanceAmount {
            dr_balance: checked_sub(
                self.details.pending.dr_balance,
                since.details.pending.dr_balance,
                account_id,
            )?,
            cr_balance: checked_sub(
                self.details.pending.cr_balance,
                since.details.pending.cr_balance,
                account_id,
            )?,
            ..self.details.pending
        };
        self.details.encumbrance = BalanceAmount {
            dr_balance: checked_sub(
                self.details.encumbrance.dr_balance,
                since.details.encumbrance.dr_balance,
                account_id,
            )?,
            cr_balance: checked_sub(
                self.details.encumbrance.cr_balance,
                since.details.encumbrance.cr_balance,
                account_id,
            )?,
            ..self.details.encumbrance
        };
        Ok(self)
    }

    /// Signed balance for the pending layer, oriented by the account's
    /// `balance_type` (credit: cr − dr; debit: dr − cr). Returns
    /// `Err(BalanceError::Overflow)` when the oriented difference exceeds the
    /// representable `Decimal` range.
    pub fn pending(&self) -> Result<Decimal, BalanceError> {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .pending()
    }

    /// Signed balance for the settled layer, oriented by the account's
    /// `balance_type` (credit: cr − dr; debit: dr − cr). Returns
    /// `Err(BalanceError::Overflow)` when the oriented difference exceeds the
    /// representable `Decimal` range.
    pub fn settled(&self) -> Result<Decimal, BalanceError> {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .settled()
    }

    /// Signed balance for the encumbrance layer, oriented by the account's
    /// `balance_type` (credit: cr − dr; debit: dr − cr). Returns
    /// `Err(BalanceError::Overflow)` when the oriented difference exceeds the
    /// representable `Decimal` range.
    pub fn encumbrance(&self) -> Result<Decimal, BalanceError> {
        BalanceWithDirection {
            direction: self.balance_type,
            details: &self.details,
        }
        .encumbrance()
    }

    /// Available balance for `layer`, composing the underlying layers: `Settled`
    /// is the settled balance alone; `Pending` adds pending to settled;
    /// `Encumbrance` adds pending and settled to encumbrance. Each addition is
    /// checked and returns `Err(BalanceError::Overflow)` if any intermediate or
    /// final sum exceeds the representable `Decimal` range.
    pub fn available(&self, layer: Layer) -> Result<Decimal, BalanceError> {
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

    pub fn pending(&self) -> Result<Decimal, BalanceError> {
        self.direction_diff(&self.details.pending)
    }

    pub fn settled(&self) -> Result<Decimal, BalanceError> {
        self.direction_diff(&self.details.settled)
    }

    pub fn encumbrance(&self) -> Result<Decimal, BalanceError> {
        self.direction_diff(&self.details.encumbrance)
    }

    pub fn available(&self, layer: Layer) -> Result<Decimal, BalanceError> {
        let account_id = self.details.account_id;
        match layer {
            Layer::Settled => self.settled(),
            Layer::Pending => {
                let sum = checked_add(self.pending()?, self.settled()?, account_id)?;
                Ok(sum)
            }
            Layer::Encumbrance => {
                let sum = checked_add(self.pending()?, self.settled()?, account_id)?;
                checked_add(sum, self.encumbrance()?, account_id)
            }
        }
    }

    // Signed difference between the two balance sides, oriented by the
    // account's normal balance type. Overflow (both sides individually
    // representable, difference not) surfaces as `BalanceError::Overflow`.
    fn direction_diff(&self, amount: &BalanceAmount) -> Result<Decimal, BalanceError> {
        let (lhs, rhs) = match self.direction {
            DebitOrCredit::Credit => (amount.cr_balance, amount.dr_balance),
            DebitOrCredit::Debit => (amount.dr_balance, amount.cr_balance),
        };
        checked_sub(lhs, rhs, self.details.account_id)
    }
}

fn checked_sub(a: Decimal, b: Decimal, account_id: AccountId) -> Result<Decimal, BalanceError> {
    a.checked_sub(b).ok_or(BalanceError::Overflow(account_id))
}

fn checked_add(a: Decimal, b: Decimal, account_id: AccountId) -> Result<Decimal, BalanceError> {
    a.checked_add(b).ok_or(BalanceError::Overflow(account_id))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal::Decimal;

    use super::*;

    fn amount(dr_balance: Decimal, cr_balance: Decimal) -> BalanceAmount {
        BalanceAmount {
            dr_balance,
            cr_balance,
            entry_id: EntryId::new(),
            modified_at: Utc::now(),
        }
    }

    fn account_balance() -> AccountBalance {
        let entry_id = EntryId::new();
        let time = Utc::now();
        let amount = BalanceAmount {
            dr_balance: Decimal::ZERO,
            cr_balance: Decimal::ZERO,
            entry_id,
            modified_at: time,
        };
        AccountBalance::new(
            DebitOrCredit::Credit,
            BalanceSnapshot {
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
                entry_id,
                currency: Currency::USD,
                settled: amount.clone(),
                pending: amount.clone(),
                encumbrance: amount,
                version: 0,
                modified_at: time,
                created_at: time,
            },
        )
    }

    fn account_balance_with_amounts(
        direction: DebitOrCredit,
        settled: BalanceAmount,
        pending: BalanceAmount,
        encumbrance: BalanceAmount,
    ) -> AccountBalance {
        AccountBalance::new(
            direction,
            BalanceSnapshot {
                journal_id: JournalId::new(),
                account_id: AccountId::new(),
                entry_id: settled.entry_id,
                currency: Currency::USD,
                settled,
                pending,
                encumbrance,
                version: 0,
                modified_at: Utc::now(),
                created_at: Utc::now(),
            },
        )
    }

    #[test]
    fn accessors_return_zero_for_zero_amounts() {
        let balance = account_balance();
        assert_eq!(balance.settled().unwrap(), Decimal::ZERO);
        assert_eq!(balance.pending().unwrap(), Decimal::ZERO);
        assert_eq!(balance.encumbrance().unwrap(), Decimal::ZERO);
        for layer in [Layer::Settled, Layer::Pending, Layer::Encumbrance] {
            assert_eq!(balance.available(layer).unwrap(), Decimal::ZERO);
        }
    }

    #[test]
    fn accessors_respect_direction() {
        let balance = account_balance_with_amounts(
            DebitOrCredit::Credit,
            amount(Decimal::ONE, Decimal::from(3)),
            amount(Decimal::TWO, Decimal::from(5)),
            amount(Decimal::from(4), Decimal::from(7)),
        );
        assert_eq!(balance.settled().unwrap(), Decimal::TWO);
        assert_eq!(balance.pending().unwrap(), Decimal::from(3));
        assert_eq!(balance.encumbrance().unwrap(), Decimal::from(3));
        assert_eq!(balance.available(Layer::Settled).unwrap(), Decimal::TWO);
        assert_eq!(balance.available(Layer::Pending).unwrap(), Decimal::from(5));
        assert_eq!(
            balance.available(Layer::Encumbrance).unwrap(),
            Decimal::from(8)
        );
    }

    #[test]
    fn accessors_overflow_is_err() {
        // Both sides are individually representable, but their difference
        // exceeds Decimal::MAX — this is the fuzz-found crash scenario.
        let credit = account_balance_with_amounts(
            DebitOrCredit::Credit,
            amount(Decimal::MIN, Decimal::MAX),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(credit.settled(), Err(BalanceError::Overflow(_))));
        assert!(matches!(
            credit.available(Layer::Settled),
            Err(BalanceError::Overflow(_))
        ));

        let debit_settled = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MAX, Decimal::MIN),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(
            debit_settled.settled(),
            Err(BalanceError::Overflow(_))
        ));

        let debit_pending = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::MAX, Decimal::MIN),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(
            debit_pending.pending(),
            Err(BalanceError::Overflow(_))
        ));

        let debit_encumbrance = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::MAX, Decimal::MIN),
        );
        assert!(matches!(
            debit_encumbrance.encumbrance(),
            Err(BalanceError::Overflow(_))
        ));
    }

    #[test]
    fn available_layer_rollup_overflow_is_err() {
        // Each layer difference is representable, but summing the layers
        // overflows — the `checked_add` in `available` must catch it.
        let balance = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MAX, Decimal::ZERO),
            amount(Decimal::ONE, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert_eq!(balance.settled().unwrap(), Decimal::MAX);
        assert_eq!(balance.pending().unwrap(), Decimal::ONE);
        assert!(matches!(
            balance.available(Layer::Pending),
            Err(BalanceError::Overflow(_))
        ));
        assert!(matches!(
            balance.available(Layer::Encumbrance),
            Err(BalanceError::Overflow(_))
        ));
    }

    #[test]
    fn derive_diff_overflow_is_err() {
        let open = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MIN, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        let close = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MAX, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(
            close.derive_diff(&open),
            Err(BalanceError::Overflow(_))
        ));
    }

    #[test]
    fn derive_diff_of_small_values_is_ok() {
        let open = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::ONE, Decimal::ZERO),
            amount(Decimal::TWO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        let close = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::from(4), Decimal::ZERO),
            amount(Decimal::from(5), Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        let diff = close.derive_diff(&open).expect("small values diff");
        assert_eq!(diff.details.settled.dr_balance, Decimal::from(3));
        assert_eq!(diff.details.pending.dr_balance, Decimal::from(3));
    }

    #[test]
    fn balance_range_new_with_start_is_ok() {
        let start = account_balance();
        let mut end = account_balance();
        end.details.version = 10;
        let range = BalanceRange::new(Some(start), end, 7).expect("zero amounts diff");
        assert_eq!(range.period.details.version, 7);
    }

    #[test]
    fn balance_range_new_without_start_is_ok() {
        let mut end = account_balance();
        end.details.version = 10;
        let range = BalanceRange::new(None, end, 7).expect("no diff without a start balance");
        assert_eq!(range.period.details.version, 7);
        assert_eq!(range.open.details.version, 0);
        assert_eq!(range.open.details.settled.dr_balance, Decimal::ZERO);
    }

    #[test]
    fn balance_range_new_overflow_is_err() {
        let start = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MIN, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        let end = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MAX, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(
            BalanceRange::new(Some(start), end, 7),
            Err(BalanceError::Overflow(_))
        ));
    }

    #[test]
    fn from_bounds_sets_period_version_to_the_diff() {
        let range =
            BalanceRange::from_bounds(Some(account_balance()), 3, Some(account_balance()), 10)
                .expect("bounds are in range")
                .expect("a close balance yields a range");
        assert_eq!(range.period.details.version, 7);
    }

    #[test]
    fn from_bounds_without_close_is_none() {
        assert!(
            BalanceRange::from_bounds(Some(account_balance()), 3, None, 10)
                .unwrap()
                .is_none()
        );
        assert!(BalanceRange::from_bounds(None, 0, None, 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn from_bounds_inverted_versions_saturate_instead_of_panicking() {
        let range =
            BalanceRange::from_bounds(Some(account_balance()), 5, Some(account_balance()), 0)
                .expect("bounds are in range")
                .expect("a close balance yields a range");
        assert_eq!(range.period.details.version, 0);
    }

    #[test]
    fn from_bounds_overflow_is_err() {
        let start = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MIN, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        let end = account_balance_with_amounts(
            DebitOrCredit::Debit,
            amount(Decimal::MAX, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
        );
        assert!(matches!(
            BalanceRange::from_bounds(Some(start), 3, Some(end), 10),
            Err(BalanceError::Overflow(_))
        ));
    }
}

#[derive(Debug, Clone)]
pub struct BalanceRange {
    pub open: AccountBalance,
    pub period: AccountBalance,
    pub close: AccountBalance,
}

impl BalanceRange {
    /// Build a range from an optional `start` balance and the `end` balance,
    /// with the period's version set to `version_diff`.
    ///
    /// When `start` is present, `period` is the checked difference
    /// `end − start` (returning `Err(BalanceError::Overflow)` on overflow),
    /// `open` is `start`, and `close` is `end`. When `start` is absent, `open`
    /// is a zero-valued open bound (version 0, zero amounts) and `period`
    /// mirrors `end` with its version set to `version_diff`.
    pub fn new(
        start: Option<AccountBalance>,
        end: AccountBalance,
        version_diff: u32,
    ) -> Result<Self, BalanceError> {
        match start {
            Some(start) => {
                let close = end.clone();
                let mut period = end.derive_diff(&start)?;
                period.details.version = version_diff;
                Ok(Self {
                    close,
                    period,
                    open: start,
                })
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
                Ok(Self {
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
                })
            }
        }
    }

    /// Build a range from its `(open, close)` bounds. Returns `Ok(None)` when
    /// there is no closing balance — i.e. the account had no activity in
    /// the window, so there is no range to report.
    ///
    /// The version diff saturates at zero: an inverted window (until < from)
    /// can pair an end snapshot older than the start snapshot, and the
    /// difference must not underflow.
    pub fn from_bounds(
        start: Option<AccountBalance>,
        start_version: u32,
        end: Option<AccountBalance>,
        end_version: u32,
    ) -> Result<Option<Self>, BalanceError> {
        match end {
            Some(end) => Self::new(start, end, end_version.saturating_sub(start_version)).map(Some),
            None => Ok(None),
        }
    }
}
