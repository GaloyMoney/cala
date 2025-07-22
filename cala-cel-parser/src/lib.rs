#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use lalrpop_util::lalrpop_mod;

pub mod ast;

pub use ast::*;

lalrpop_mod!(#[allow(clippy::all)] pub parser, "/cel.rs");
