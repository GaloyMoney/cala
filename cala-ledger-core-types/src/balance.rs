use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceSnapshot {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub entry_id: EntryId,
    pub settled: BalanceAmount,
    pub pending: BalanceAmount,
    pub encumbrance: BalanceAmount,
}

impl BalanceSnapshot {
    /// Available balance for `layer`, rolling up the underlying layers with
    /// checked arithmetic.
    ///
    /// Returns [`BalanceAmountOverflow`] when the per-layer rollup exceeds
    /// the representable range of [`Decimal`]; individually-representable
    /// layer values can still produce an unrepresentable sum.
    pub fn available(&self, layer: Layer) -> Result<BalanceAmount, BalanceAmountOverflow> {
        match layer {
            Layer::Settled => Ok(self.settled.clone()),
            Layer::Pending => self.settled.rollup(&self.pending),
            Layer::Encumbrance => self
                .settled
                .rollup(&self.pending)?
                .rollup(&self.encumbrance),
        }
    }
}

/// Error returned when a balance computation exceeds the representable range
/// of a [`Decimal`] (96-bit mantissa).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("BalanceAmountOverflow: result exceeds the representable decimal range")]
pub struct BalanceAmountOverflow;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceAmount {
    pub dr_balance: Decimal,
    pub cr_balance: Decimal,
    pub entry_id: EntryId,
    pub modified_at: DateTime<Utc>,
}

impl BalanceAmount {
    /// Sum two per-layer amounts, keeping the most recent `entry_id` /
    /// `modified_at`. Uses checked addition so that overflow surfaces as an
    /// error instead of a panic.
    fn rollup(&self, other: &Self) -> Result<Self, BalanceAmountOverflow> {
        let (modified_at, entry_id) = if self.modified_at >= other.modified_at {
            (self.modified_at, self.entry_id)
        } else {
            (other.modified_at, other.entry_id)
        };

        Ok(Self {
            dr_balance: self
                .dr_balance
                .checked_add(other.dr_balance)
                .ok_or(BalanceAmountOverflow)?,
            cr_balance: self
                .cr_balance
                .checked_add(other.cr_balance)
                .ok_or(BalanceAmountOverflow)?,
            entry_id,
            modified_at,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectiveBalanceSnapshot {
    pub journal_id: JournalId,
    pub account_id: AccountId,
    pub currency: Currency,
    pub effective: NaiveDate,
    pub version: u32,
    pub all_time_version: u32,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub entry_id: EntryId,
    pub settled: BalanceAmount,
    pub pending: BalanceAmount,
    pub encumbrance: BalanceAmount,
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

    fn snapshot(
        settled: BalanceAmount,
        pending: BalanceAmount,
        encumbrance: BalanceAmount,
    ) -> BalanceSnapshot {
        BalanceSnapshot {
            journal_id: JournalId::new(),
            account_id: AccountId::new(),
            currency: Currency::USD,
            version: 0,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            entry_id: settled.entry_id,
            settled,
            pending,
            encumbrance,
        }
    }

    #[test]
    fn rollup_within_range_is_ok() {
        let a = amount(Decimal::ONE, Decimal::TWO);
        let b = amount(Decimal::TWO, Decimal::ONE);
        let rolled = a.rollup(&b).expect("small values roll up");
        assert_eq!(rolled.dr_balance, Decimal::from(3));
        assert_eq!(rolled.cr_balance, Decimal::from(3));
    }

    #[test]
    fn rollup_overflow_is_err() {
        // Individually representable values whose sum exceeds Decimal::MAX.
        let a = amount(Decimal::MAX, Decimal::ZERO);
        let b = amount(Decimal::ONE, Decimal::ZERO);
        assert_eq!(a.rollup(&b), Err(BalanceAmountOverflow));
    }

    #[test]
    fn rollup_negative_overflow_is_err() {
        let a = amount(Decimal::MIN, Decimal::ZERO);
        let b = amount(Decimal::NEGATIVE_ONE, Decimal::ZERO);
        assert_eq!(a.rollup(&b), Err(BalanceAmountOverflow));
    }

    #[test]
    fn available_settled_is_unchanged() {
        let settled = amount(Decimal::ONE, Decimal::TWO);
        let snap = snapshot(
            settled.clone(),
            amount(Decimal::MAX, Decimal::MAX),
            amount(Decimal::MAX, Decimal::MAX),
        );
        assert_eq!(snap.available(Layer::Settled), Ok(settled));
    }

    #[test]
    fn available_pending_rolls_up_settled_and_pending() {
        let snap = snapshot(
            amount(Decimal::ONE, Decimal::TWO),
            amount(Decimal::TWO, Decimal::ONE),
            amount(Decimal::MAX, Decimal::MAX),
        );
        let available = snap
            .available(Layer::Pending)
            .expect("small values roll up");
        assert_eq!(available.dr_balance, Decimal::from(3));
        assert_eq!(available.cr_balance, Decimal::from(3));
    }

    #[test]
    fn available_encumbrance_rolls_up_all_layers() {
        let snap = snapshot(
            amount(Decimal::ONE, Decimal::ZERO),
            amount(Decimal::TWO, Decimal::ZERO),
            amount(Decimal::from(3), Decimal::ZERO),
        );
        let available = snap
            .available(Layer::Encumbrance)
            .expect("small values roll up");
        assert_eq!(available.dr_balance, Decimal::from(6));
        assert_eq!(available.cr_balance, Decimal::ZERO);
    }

    #[test]
    fn available_overflow_is_err() {
        // Settled + pending is representable, but the encumbrance rollup
        // overflows — `available` must surface the error transitively.
        let snap = snapshot(
            amount(Decimal::MAX, Decimal::ZERO),
            amount(Decimal::ZERO, Decimal::ZERO),
            amount(Decimal::ONE, Decimal::ZERO),
        );
        let settled = snap
            .available(Layer::Settled)
            .expect("settled alone is in range");
        assert_eq!(settled.dr_balance, Decimal::MAX);
        let pending = snap
            .available(Layer::Pending)
            .expect("settled + zero pending is in range");
        assert_eq!(pending.dr_balance, Decimal::MAX);
        assert_eq!(
            snap.available(Layer::Encumbrance),
            Err(BalanceAmountOverflow)
        );
    }
}
