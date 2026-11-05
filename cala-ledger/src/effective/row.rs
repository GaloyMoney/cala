use chrono::{DateTime, NaiveDate, Utc};
use es_entity::errlanes::{lanes, Fault, ResultExt};
use rust_decimal::Decimal;

use cala_types::{
    balance::{BalanceAmount, BalanceSnapshot, EffectiveBalanceSnapshot},
    primitives::{AccountId, Currency, DebitOrCredit, EntryId, JournalId},
};

use crate::balance::AccountBalance;

/// The columns an effective series row shares across both tables, qualified
/// with the table alias `c`. Every read query selects this list (the
/// calculation table appends its own key columns) so that a single
/// [`EffectiveRow`] mapper serves them all.
macro_rules! effective_columns {
    () => {
        "c.journal_id, c.account_id, c.currency, c.effective, c.version, \
         c.all_time_version, c.latest_entry_id, \
         c.settled_dr_balance, c.settled_cr_balance, c.settled_entry_id, c.settled_modified_at, \
         c.pending_dr_balance, c.pending_cr_balance, c.pending_entry_id, c.pending_modified_at, \
         c.encumbrance_dr_balance, c.encumbrance_cr_balance, c.encumbrance_entry_id, \
         c.encumbrance_modified_at, c.updated_at, c.created_at"
    };
}
pub(crate) use effective_columns;

/// One stored row: the cumulative balance of a series as of the end of one
/// effective date.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct EffectiveRow {
    journal_id: JournalId,
    account_id: AccountId,
    currency: String,
    effective: NaiveDate,
    version: i32,
    all_time_version: i32,
    latest_entry_id: EntryId,
    settled_dr_balance: Decimal,
    settled_cr_balance: Decimal,
    settled_entry_id: EntryId,
    settled_modified_at: DateTime<Utc>,
    pending_dr_balance: Decimal,
    pending_cr_balance: Decimal,
    pending_entry_id: EntryId,
    pending_modified_at: DateTime<Utc>,
    encumbrance_dr_balance: Decimal,
    encumbrance_cr_balance: Decimal,
    encumbrance_entry_id: EntryId,
    encumbrance_modified_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl EffectiveRow {
    /// Entries folded into the series since its first row.
    pub(crate) fn all_time_version(&self) -> u32 {
        self.all_time_version as u32
    }

    pub(crate) fn into_snapshot(self) -> Result<EffectiveBalanceSnapshot, Fault<lanes!(Fatal)>> {
        let currency: Currency = self
            .currency
            .parse()
            .classify::<crate::error::CouldNotDecodeCurrency>()?;
        Ok(EffectiveBalanceSnapshot {
            journal_id: self.journal_id,
            account_id: self.account_id,
            currency,
            effective: self.effective,
            version: self.version as u32,
            all_time_version: self.all_time_version as u32,
            created_at: self.created_at,
            modified_at: self.updated_at,
            entry_id: self.latest_entry_id,
            settled: BalanceAmount {
                dr_balance: self.settled_dr_balance,
                cr_balance: self.settled_cr_balance,
                entry_id: self.settled_entry_id,
                modified_at: self.settled_modified_at,
            },
            pending: BalanceAmount {
                dr_balance: self.pending_dr_balance,
                cr_balance: self.pending_cr_balance,
                entry_id: self.pending_entry_id,
                modified_at: self.pending_modified_at,
            },
            encumbrance: BalanceAmount {
                dr_balance: self.encumbrance_dr_balance,
                cr_balance: self.encumbrance_cr_balance,
                entry_id: self.encumbrance_entry_id,
                modified_at: self.encumbrance_modified_at,
            },
        })
    }

    fn into_balance_snapshot(self) -> Result<BalanceSnapshot, Fault<lanes!(Fatal)>> {
        let EffectiveBalanceSnapshot {
            journal_id,
            account_id,
            currency,
            version,
            created_at,
            modified_at,
            entry_id,
            settled,
            pending,
            encumbrance,
            ..
        } = self.into_snapshot()?;
        Ok(BalanceSnapshot {
            journal_id,
            account_id,
            currency,
            version,
            created_at,
            modified_at,
            entry_id,
            settled,
            pending,
            encumbrance,
        })
    }
}

/// A stored row together with the owning account's normal balance type.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct AccountEffectiveRow {
    #[sqlx(flatten)]
    row: EffectiveRow,
    normal_balance_type: DebitOrCredit,
}

impl AccountEffectiveRow {
    /// Entries folded into the series since its first row.
    pub(crate) fn all_time_version(&self) -> u32 {
        self.row.all_time_version()
    }

    pub(crate) fn into_account_balance(self) -> Result<AccountBalance, Fault<lanes!(Fatal)>> {
        Ok(AccountBalance::new(
            self.normal_balance_type,
            self.row.into_balance_snapshot()?,
        ))
    }
}
