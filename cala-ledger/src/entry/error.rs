use thiserror::Error;

use super::repo::{EntryCreateError, EntryFindError, EntryModifyError, EntryQueryError};

/// Composite FK forbidding a `cala_entries` row from targeting an
/// account-set backing account (#802). Hand-written in the migrations,
/// so es-entity's schema parsing does not model it — `ConstraintViolation`
/// is generated for unique violations only; FK violations surface as the
/// raw `Sqlx` error and are mapped to a domain variant by constraint
/// name below, per the house pattern (cf. `AccountSetError::MemberAlreadyAdded`).
const ENTRY_ACCOUNT_SET_FK: &str = "cala_entries_account_not_account_set_fkey";

#[derive(Error, Debug)]
pub enum EntryError {
    #[error("EntryError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("EntryError - Create: {0}")]
    Create(EntryCreateError),
    #[error("EntryError - Modify: {0}")]
    Modify(#[from] EntryModifyError),
    #[error("EntryError - Find: {0}")]
    Find(#[from] EntryFindError),
    #[error("EntryError - Query: {0}")]
    Query(#[from] EntryQueryError),
    #[error(
        "EntryError - EntryTargetsAccountSet: an entry may not be posted directly to an \
         account-set backing account; an account set's balance is derived from its members"
    )]
    EntryTargetsAccountSet,
}

fn violates_entry_account_set_fk(e: &sqlx::Error) -> bool {
    e.as_database_error().and_then(|db| db.constraint()) == Some(ENTRY_ACCOUNT_SET_FK)
}

impl From<EntryCreateError> for EntryError {
    fn from(error: EntryCreateError) -> Self {
        match error {
            // es-entity (as of 0.12) classifies only the entity's own
            // recognized *unique* constraints as `ConstraintViolation`;
            // any other constraint violation — this hand-written FK
            // included — falls through as raw `Sqlx` (verified: the
            // `rejects_direct_entry_to_account_set` test passes via this
            // arm). That classification gap should be fixed upstream so
            // every named-constraint violation surfaces as
            // `ConstraintViolation { column: None, inner }`; the second
            // arm already handles that shape so the mapping survives the
            // upstream fix unchanged.
            EntryCreateError::Sqlx(e) if violates_entry_account_set_fk(&e) => {
                Self::EntryTargetsAccountSet
            }
            EntryCreateError::ConstraintViolation { inner, .. }
                if violates_entry_account_set_fk(&inner) =>
            {
                Self::EntryTargetsAccountSet
            }
            other => Self::Create(other),
        }
    }
}
