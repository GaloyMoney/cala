#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

pub mod ast;
#[allow(clippy::all)]
pub mod parser;

pub use ast::*;
