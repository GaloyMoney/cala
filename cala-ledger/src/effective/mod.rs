//! Effective-balance series mechanics shared by the account series
//! (`balance::effective`, table `cala_cumulative_effective_balances`) and
//! the calculation series (`calculation::effective`, table
//! `cala_calculation_effective_balances`).
//!
//! A series is one cumulative balance per effective date. The account table
//! has one series per `(account, currency)`; the calculation table has one
//! per `(calculation, dimensions, account, currency)`. Everything here is
//! independent of which: the stored row shape ([`row`]), the per-date delta
//! a batch folds before it is written ([`delta`]), the cumulative-at-dates
//! fold ([`cumulative`]) and its projection onto whole periods
//! ([`periods`]). The public value types are re-exported from
//! `cala_ledger::balance`.

pub(crate) mod cumulative;
pub(crate) mod delta;
pub(crate) mod periods;
pub(crate) mod row;
