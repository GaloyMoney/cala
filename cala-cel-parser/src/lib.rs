#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use cached::proc_macro::cached;
use tracing::instrument;

pub mod ast;

pub use ast::*;

// Pre-generated LALRPOP parser - regenerate with:
//   cargo build -p cala-cel-parser --features regenerate-parser
//   cp target/debug/build/cala-cel-parser-*/out/cel.rs cala-cel-parser/src/cel.rs
#[allow(clippy::all)]
mod parser {
    include!("cel.rs");
}

/// Error type for batch parsing
#[derive(Debug)]
pub struct ParseErrors(pub Vec<(usize, String)>);

impl std::fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} parse error(s)", self.0.len())
    }
}

impl std::error::Error for ParseErrors {}

/// Instrumented wrapper for parsing CEL expressions with caching
///
/// This provides visibility into parsing operations while using the
/// LALRPOP-generated parser internally. Parsing results are cached
/// to avoid re-parsing the same expression multiple times.
#[cached(size = 10000)]
#[instrument(name = "cel.parse", skip(source), fields(expression = %source), err)]
pub fn parse_expression(source: String) -> Result<Expression, String> {
    parser::ExpressionParser::new()
        .parse(&source)
        .map_err(|e| e.to_string())
}
