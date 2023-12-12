#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

mod account;
mod generic_error;
mod ledger;
mod queries;

pub(crate) use generic_error::generic_napi_error;
