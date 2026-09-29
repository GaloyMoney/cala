use rust_decimal::Decimal;
use thiserror::Error;

use cel_interpreter::CelError;

use crate::error_support::{impl_lane_error_fail, impl_lane_error_fault_only};
use crate::param::error::ParamRejection;
use crate::primitives::*;

use super::control::{VelocityControlConstraint, VelocityControlConstraintViolation};
use super::limit::{VelocityLimitConstraint, VelocityLimitConstraintViolation};

// Not `Clone`: carries `CelError` (from `cala-cel-interpreter`, out of this
// rollout's scope), which does not implement it.
#[derive(Debug, Error, errlanes::Rejection)]
#[rejection(lift(VelocityControlConstraintViolation, VelocityLimitConstraintViolation))]
pub enum VelocityRejection {
    /// Not `#[from]`: see `param::error::ParamRejection::CelError` — the
    /// wrapped type would need to implement `errlanes::Rejection`, and
    /// `CelError` is out of this rollout's scope.
    #[error("VelocityError - CelError: {0}")]
    CelError(CelError),
    #[error(transparent)]
    Param(#[from] ParamRejection),
    #[error("velocity control '{0}' not found")]
    NotFoundControlById(VelocityControlId),
    /// #5800: keeps `LimitExceededError`'s fields (limit/control ids) so a
    /// consumer can match one level deep and learn *which* control
    /// rejected, instead of matching four levels into an opaque
    /// `PostingError`/`VelocityError` nest.
    #[error("VelocityError - Enforcement: {0}")]
    Enforcement(LimitExceededError),
    #[error("control_id '{0}' already exists")]
    #[rejection(
        key = VelocityControlConstraint::Pkey,
        via = VelocityControlConstraintViolation,
        with = control_id_taken
    )]
    ControlIdAlreadyExists(String),
    #[error("limit_id '{0}' already exists")]
    #[rejection(
        key = VelocityLimitConstraint::Pkey,
        via = VelocityLimitConstraintViolation,
        with = limit_id_taken
    )]
    LimitIdAlreadyExists(String),
    #[error("limit already added to control")]
    LimitAlreadyAddedToControl,
}

fn control_id_taken(cv: VelocityControlConstraintViolation) -> VelocityRejection {
    VelocityRejection::ControlIdAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

fn limit_id_taken(cv: VelocityLimitConstraintViolation) -> VelocityRejection {
    VelocityRejection::LimitIdAlreadyExists(cv.value().unwrap_or_default().to_owned())
}

#[derive(Debug, Error)]
pub enum VelocityError {
    #[error(transparent)]
    Rejected(#[from] VelocityRejection),
    #[error(transparent)]
    Transient(#[from] errlanes::Transient),
    #[error(transparent)]
    Fatal(#[from] errlanes::Fatal),
}

impl_lane_error_fault_only!(VelocityError);
impl_lane_error_fail!(
    VelocityError,
    VelocityRejection,
    VelocityControlConstraintViolation
);
impl_lane_error_fail!(
    VelocityError,
    VelocityRejection,
    VelocityLimitConstraintViolation
);

/// `cala_velocity_control_limits` is a plain join table (no `EsRepo` of its
/// own), so its unique-pair violation — a limit already attached to a
/// control — cannot come through the generated `ConstraintViolation`/`Lift`
/// path the way `ControlIdAlreadyExists`/`LimitIdAlreadyExists` do. The
/// name below is confirmed against the live schema (`\d
/// cala_velocity_control_limits` — Postgres's 63-byte identifier truncation
/// makes this unguessable from the migration source alone), matched
/// exactly rather than by substring.
const VELOCITY_CONTROL_LIMITS_UNIQUE_CONSTRAINT: &str =
    "cala_velocity_control_limits_velocity_control_id_velocity_l_key";

impl From<sqlx::Error> for VelocityError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some(VELOCITY_CONTROL_LIMITS_UNIQUE_CONSTRAINT) {
                return VelocityRejection::LimitAlreadyAddedToControl.into();
            }
        }
        errlanes::Fault::from(e).into()
    }
}

/// A stored velocity-context snapshot that no longer hydrates is corrupt
/// state, not a caller-correctable outcome.
impl From<es_entity::EntityHydrationError> for VelocityError {
    fn from(e: es_entity::EntityHydrationError) -> Self {
        Self::Fatal(errlanes::Fatal::from_error(
            errlanes::FatalKind::CorruptState,
            e,
        ))
    }
}

impl From<CelError> for VelocityError {
    fn from(e: CelError) -> Self {
        Self::Rejected(VelocityRejection::CelError(e))
    }
}

impl From<ParamRejection> for VelocityError {
    fn from(e: ParamRejection) -> Self {
        Self::Rejected(VelocityRejection::Param(e))
    }
}

impl From<LimitExceededError> for VelocityError {
    fn from(e: LimitExceededError) -> Self {
        Self::Rejected(VelocityRejection::Enforcement(e))
    }
}

#[derive(Debug, Clone, Error)]
#[error("Velocity limit exceeded")]
pub struct LimitExceededError {
    pub account_id: AccountId,
    pub currency: Currency,
    pub limit_id: VelocityLimitId,
    pub layer: Layer,
    pub direction: DebitOrCredit,
    pub limit: Decimal,
    pub requested: Decimal,
}
