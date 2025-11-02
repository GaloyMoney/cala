#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use lalrpop_util::lalrpop_mod;
use tracing::instrument;

pub mod ast;

pub use ast::*;

// Private module - use parse_expression() instead
lalrpop_mod!(
    #[allow(clippy::all)]
    parser,
    "/cel.rs"
);

/// Error type for batch parsing
#[derive(Debug)]
pub struct ParseErrors(pub Vec<(usize, String)>);

impl std::fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} parse error(s)", self.0.len())
    }
}

impl std::error::Error for ParseErrors {}

/// Instrumented wrapper for parsing CEL expressions
///
/// This provides visibility into parsing operations while using the
/// LALRPOP-generated parser internally.
#[instrument(name = "cel.parse", skip(source), fields(expression = %source), err)]
pub fn parse_expression(source: &str) -> Result<Expression, String> {
    parser::ExpressionParser::new()
        .parse(source)
        .map_err(|e| e.to_string())
}
