//! Shared boilerplate for every module's lane-shaped `{Module}Error`.
//!
//! Each domain error is `{ Rejected(XRejection), Transient, Fatal }` — see
//! the module docs on any `error.rs` for the shape. The `From` impls every
//! such type needs (from `errlanes::Fault`, from `sqlx::Error`, and from
//! `errlanes::Fail<XConstraintViolation>` when the module has one) are
//! identical modulo the types involved, so they live here once instead of
//! being retyped in every `error.rs`.

/// cala issues no authorization denials of its own, so every conversion
/// below maps `errlanes::Denied` to `Fatal(Invariant)` rather than
/// `unreachable!()` — if something upstream ever does start producing one,
/// this degrades to an operator page instead of a panic.
pub(crate) fn denied_is_unreachable() -> errlanes::Fatal {
    errlanes::Fatal::invariant("unexpected authorization denial")
}

/// Implements `From<errlanes::Fault>` only. Use together with a hand-written
/// `From<sqlx::Error>` for a module that intercepts specific constraint
/// names (duplicate-key sniffing, etc.) before falling back to
/// `errlanes::Fault::from(e).into()`; [`impl_lane_error_fault`] covers the
/// common case of both at once.
macro_rules! impl_lane_error_fault_only {
    ($err:ty) => {
        impl From<errlanes::Fault> for $err {
            fn from(f: errlanes::Fault) -> Self {
                match f {
                    errlanes::Fault::Transient(t) => Self::Transient(t),
                    errlanes::Fault::Fatal(x) => Self::Fatal(x),
                    errlanes::Fault::Denied(_) => {
                        Self::Fatal(crate::error_support::denied_is_unreachable())
                    }
                }
            }
        }
    };
}

/// Implements `From<errlanes::Fault>` and `From<sqlx::Error>` for a
/// lane-shaped error type. Use on its own for a module whose
/// `From<sqlx::Error>` needs no special-casing; a module that intercepts
/// specific constraint names (duplicate-key sniffing, etc.) uses
/// [`impl_lane_error_fault_only`] instead and writes `From<sqlx::Error>` by
/// hand, routing its fallback case through `errlanes::Fault::from(e).into()`.
macro_rules! impl_lane_error_fault {
    ($err:ty) => {
        crate::error_support::impl_lane_error_fault_only!($err);

        impl From<sqlx::Error> for $err {
            fn from(e: sqlx::Error) -> Self {
                errlanes::Fault::from(e).into()
            }
        }
    };
}

/// Implements `From<errlanes::Fail<$violation>>` for a lane-shaped error
/// type, widening the constraint violation through `$rejection`'s
/// `errlanes::Lift` impl (an unlisted key demotes to `Fatal(Invariant)` —
/// see `errlanes::Rejection`'s `key` docs).
macro_rules! impl_lane_error_fail {
    ($err:ty, $rejection:ty, $violation:ty) => {
        impl From<errlanes::Fail<$violation>> for $err {
            fn from(f: errlanes::Fail<$violation>) -> Self {
                match f.widen_with(<$rejection as errlanes::Lift<_>>::lift) {
                    errlanes::Fail::Rejected(r) => Self::Rejected(r),
                    errlanes::Fail::Transient(t) => Self::Transient(t),
                    errlanes::Fail::Fatal(x) => Self::Fatal(x),
                    errlanes::Fail::Denied(_) => {
                        Self::Fatal(crate::error_support::denied_is_unreachable())
                    }
                }
            }
        }
    };
}

pub(crate) use impl_lane_error_fail;
pub(crate) use impl_lane_error_fault;
pub(crate) use impl_lane_error_fault_only;
