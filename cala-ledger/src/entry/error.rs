use thiserror::Error;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault};

use super::repo::{EntryConstraint, EntryConstraintViolation};

#[derive(Debug, Clone, Error, errlanes::Rejection)]
#[rejection(lift(EntryConstraintViolation))]
pub enum EntryRejection {
    #[error(
        "an entry may not be posted directly to an account-set backing account; \
         an account set's balance is derived from its members"
    )]
    #[rejection(key = EntryConstraint::AccountNotAccountSetFkey)]
    EntryTargetsAccountSet,
}

#[derive(Debug, Error)]
pub enum EntryError {
    #[error(transparent)]
    Rejected(#[from] EntryRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault!(EntryError);
impl_lane_error_fail!(EntryError, EntryRejection, EntryConstraintViolation);
