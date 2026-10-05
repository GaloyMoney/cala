use std::sync::Arc;

use cached::cached;
use cel::Program;
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Stack size for the dedicated CEL compilation thread.
///
/// The `cel` crate parses with an ANTLR-generated recursive-descent parser
/// that carries no stack guard and consumes large amounts of stack in debug
/// builds: ~350KiB for a trivial two-operator expression and >8MiB for
/// expressions at its own grammar-recursion cap (96). 32MiB gives the worst
/// accepted input a >2x margin. The reservation is virtual — only pages that
/// are actually touched get committed.
const COMPILE_STACK_BYTES: usize = 32 * 1024 * 1024;

/// Globally memoized CEL program compilation.
///
/// `CelExpression`s are frequently re-created from the same source string
/// (e.g. velocity controls deserialized from the DB on every transaction),
/// so compilation results are cached to avoid re-compiling the same
/// expression multiple times.
///
/// Compilation runs on a dedicated thread with a fixed, known-large stack so
/// that success never depends on how much stack the *caller* has left —
/// compilation regularly runs on top of deep async state machines whose debug
/// frames leave far less headroom than the parser needs, so compiling on the
/// caller's thread can overflow the stack. The spawn cost is paid once per
/// unique expression thanks to the memoization above.
#[cached(max_size = 10000)]
#[instrument(name = "cel.compile", skip(source), fields(expression = %source), err(level = tracing::Level::WARN))]
fn compile_program(source: String) -> Result<Arc<Program>, CelParseRejection> {
    let started = std::time::Instant::now();
    let expression = source.clone();
    let result = std::thread::Builder::new()
        .name("cel-compile".to_string())
        .stack_size(COMPILE_STACK_BYTES)
        .spawn(move || {
            Program::compile(&source)
                .map(Arc::new)
                .map_err(|e| CelParseRejection::ParseError(e.to_string()))
        })
        .expect("failed to spawn cel-compile thread")
        .join()
        .unwrap_or_else(|panic| {
            // A library must not crash its caller: surface panics from
            // the compile thread as parse errors.
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(CelParseRejection::ParseError(format!(
                "CEL parser panicked during compilation: {msg}"
            )))
        });
    tracing::debug!(
        expression = %expression,
        elapsed = ?started.elapsed(),
        "compiled CEL program on dedicated thread"
    );
    result
}

use crate::{context::*, error::*, value::*};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct CelExpression {
    source: String,
    #[serde(skip)]
    program: Arc<Program>,
}

impl CelExpression {
    /// Evaluate and convert to a target using the shared CEL conversion contract.
    ///
    /// Targets implement `TryFrom<CelResult>` with an error convertible into
    /// `CelConversionRejection`; no target-specific evaluation family is needed.
    pub fn try_evaluate<'a, T>(&'a self, ctx: &CelContext) -> Result<T, CelConversionRejection>
    where
        T: TryFrom<CelResult<'a>>,
        CelConversionRejection: From<T::Error>,
    {
        let res = self.evaluate(ctx)?;
        Ok(T::try_from(CelResult {
            expr: &self.source,
            val: res,
        })?)
    }

    #[instrument(name = "cel.evaluate", skip_all, fields(expression = %self.source, context = tracing::field::Empty, result = tracing::field::Empty), err(level = tracing::Level::WARN))]
    pub fn evaluate(&self, ctx: &CelContext) -> Result<CelValue, CelConversionRejection> {
        let context_debug = ctx.debug_context();
        if !context_debug.is_empty() {
            tracing::Span::current().record("context", &context_debug);
        }

        let value = self
            .program
            .execute(ctx.inner())
            .map_err(|e| CelConversionRejection::from_execution(e, &self.source))?;
        let result = CelValue::from_cel_value(value, &self.source)?;

        tracing::Span::current().record("result", format!("{:?}", result));

        Ok(result)
    }
}

impl std::fmt::Display for CelExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl From<CelExpression> for String {
    fn from(expr: CelExpression) -> Self {
        expr.source
    }
}

impl TryFrom<String> for CelExpression {
    type Error = CelParseRejection;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        // Checked before compiling: oversized sources can panic cel's error
        // formatter (see `MAX_EXPRESSION_BYTES`) and would otherwise occupy
        // the byte-unbounded compile cache.
        if source.len() > MAX_EXPRESSION_BYTES {
            return Err(CelParseRejection::ExpressionTooLarge(source.len()));
        }
        let program = compile_program(source.clone())?;
        Ok(Self { source, program })
    }
}

impl TryFrom<&str> for CelExpression {
    type Error = CelParseRejection;

    fn try_from(source: &str) -> Result<Self, Self::Error> {
        Self::try_from(source.to_string())
    }
}

impl std::str::FromStr for CelExpression {
    type Err = CelParseRejection;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::try_from(source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn parser_panic_surfaces_as_error() {
        // A panic during compilation must surface as a parse error, not
        // propagate to the caller.
        let mut source = String::from(">");
        source.push_str(&"{".repeat(63));
        source.push_str("?[");
        source.push_str(&"(".repeat(11));
        source.push_str(&"{".repeat(26));
        source.push_str("l\u{0}?(-");

        let err = source.parse::<CelExpression>().unwrap_err();
        assert!(matches!(err, CelParseRejection::ParseError(_)));
    }

    #[test]
    fn oversized_expression_rejected_before_parser() {
        // The input class the `cel_compile` fuzz target found: one very long
        // line of nested parens — see `MAX_EXPRESSION_BYTES`.
        let source = "1+".to_string() + &"(".repeat(MAX_EXPRESSION_BYTES + 1);
        assert!(source.len() > MAX_EXPRESSION_BYTES);

        let err = source.parse::<CelExpression>().unwrap_err();
        assert!(matches!(err, CelParseRejection::ExpressionTooLarge(n) if n == source.len()));
    }

    #[test]
    fn long_but_legal_expression_still_compiles() {
        // A string literal is a single token: a near-limit operator chain
        // would instead recurse the parser and overflow the compile stack
        // (`MAX_EXPRESSION_BYTES` does not bound parser recursion).
        let source = format!("\"{}\"", "a".repeat(MAX_EXPRESSION_BYTES - 2));
        assert_eq!(source.len(), MAX_EXPRESSION_BYTES);

        let expr = source.parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(
            expr.evaluate(&context).unwrap(),
            CelValue::String("a".repeat(MAX_EXPRESSION_BYTES - 2).into())
        );
    }

    #[test]
    fn literals() {
        let expression = "true".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        let expression = "1".parse::<CelExpression>().unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(1));

        let expression = "-1".parse::<CelExpression>().unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(-1));

        let expression = "'hello'".parse::<CelExpression>().unwrap();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::String("hello".to_string().into())
        );
    }

    #[test]
    fn logic() {
        let expression = "true || false ? false && true : true"
            .parse::<CelExpression>()
            .unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Bool(false)
        );
        let expression = "true && false ? false : true || false"
            .parse::<CelExpression>()
            .unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true))
    }

    #[test]
    fn lookup() {
        let expression = "params.hello.world".parse::<CelExpression>().unwrap();
        let mut hello = CelMap::new();
        hello.insert("world", 42);
        let mut params = CelMap::new();
        params.insert("hello", hello);
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(42));
    }

    #[test]
    fn to_level_function() {
        let expression = "date('2022-10-10')".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        let result: NaiveDate = expression.try_evaluate(&context).unwrap();
        assert_eq!(
            result,
            NaiveDate::parse_from_str("2022-10-10", "%Y-%m-%d").unwrap()
        );
    }

    #[test]
    fn cast_function() {
        let expression = "decimal('1')".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Decimal(1.into())
        );
    }

    #[test]
    fn package_function() -> anyhow::Result<()> {
        let expression = "decimal.Add(decimal('1'), decimal('2'))"
            .parse::<CelExpression>()
            .unwrap();
        let context = CelContext::new();
        assert_eq!(expression.evaluate(&context)?, CelValue::Decimal(3.into()));
        Ok(())
    }

    #[test]
    fn decimal_arithmetic_functions() -> anyhow::Result<()> {
        let context = CelContext::new();

        let expression = "decimal.Sub(decimal('3'), decimal('1'))".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Decimal(2.into()));

        let expression = "decimal.Mul(decimal('2.5'), decimal('4'))".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Decimal(10.into()));

        // args coerce like `decimal()` does (ints, strings)
        let expression = "decimal.Sub('3', 1)".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Decimal(2.into()));

        Ok(())
    }

    #[test]
    fn decimal_cmp_function() -> anyhow::Result<()> {
        let context = CelContext::new();

        let expression = "decimal.Cmp(decimal('2'), decimal('1'))".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Int(1));

        let expression = "decimal.Cmp(decimal('1'), decimal('2'))".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Int(-1));

        // equal across scales
        let expression = "decimal.Cmp(decimal('1.0'), decimal('1'))".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Int(0));

        // composes with native int comparisons: a > b
        let expression = "decimal.Cmp(decimal('2'), decimal('1')) > 0".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Bool(true));

        // a <= b
        let expression = "decimal.Cmp(decimal('2'), decimal('1')) <= 0".parse::<CelExpression>()?;
        assert_eq!(expression.evaluate(&context)?, CelValue::Bool(false));

        Ok(())
    }

    #[test]
    fn has_macro_with_map() {
        let expression = "has(params.hello)".parse::<CelExpression>().unwrap();
        let mut params = CelMap::new();
        params.insert("hello", 42);
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        let expression = "has(params.missing)".parse::<CelExpression>().unwrap();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Bool(false)
        );
    }

    #[test]
    fn invalid_format_on_timestamp_is_error_not_panic() {
        // Regression test (found by fuzzing): chrono's `DelayedFormat`
        // `Display` impl returns `fmt::Error` for unknown specifiers like
        // `%Q`; formatting must surface a CEL evaluation error, not panic.
        let expression = "now.format('%Q')".parse::<CelExpression>().unwrap();
        let mut context = CelContext::new();
        context.add_variable(
            "now",
            chrono::NaiveDate::from_ymd_opt(1940, 12, 21)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        );
        let err = expression.evaluate(&context).unwrap_err();
        assert!(matches!(err, CelConversionRejection::Unexpected { .. }));
    }

    #[test]
    fn bytes_to_json_is_error_not_panic() {
        // Regression test (found by fuzzing): coercing a bytes value (e.g.
        // from `{'a': b': x'}`) to serde_json::Value used to hit
        // `unimplemented!()`. It must return a coercion error instead.
        let expression = "{'a': b'x'}".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        let res: Result<serde_json::Value, _> = expression.try_evaluate(&context);
        assert!(res.is_err());
    }

    #[test]
    fn function_on_timestamp() -> anyhow::Result<()> {
        let expression = "now.format('%d/%m/%Y')".parse::<CelExpression>().unwrap();
        let mut context = CelContext::new();
        context.add_variable(
            "now",
            chrono::NaiveDate::from_ymd_opt(1940, 12, 21)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        );
        assert_eq!(expression.evaluate(&context)?, CelValue::from("21/12/1940"));
        Ok(())
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::CelType;
    use es_entity::errlanes::{Level, Rejection};
    use std::error::Error;

    fn conversion_contract(error: CelConversionRejection) {
        match error {
            CelConversionRejection::CoreTypeCoercion(_)
            | CelConversionRejection::ExternalTypeCoercion(_)
            | CelConversionRejection::ExternalParse(_)
            | CelConversionRejection::Json(_)
            | CelConversionRejection::UnknownIdent { .. }
            | CelConversionRejection::MissingArgument { .. }
            | CelConversionRejection::NoMatchingOverload { .. }
            | CelConversionRejection::Unexpected { .. }
            | CelConversionRejection::UnsupportedOpaque { .. }
            | CelConversionRejection::OpaqueDowncast { .. }
            | CelConversionRejection::FunctionValue { .. } => {}
        }
    }

    #[test]
    fn scalar_evaluation_is_bare_and_keeps_expression_and_foreign_source() {
        let expr: CelExpression = "missing_variable".parse().unwrap();
        let context = CelContext::new();
        let results: [Result<(), CelConversionRejection>; 2] = [
            expr.evaluate(&context).map(|_| ()),
            expr.try_evaluate::<bool>(&context).map(|_| ()),
        ];
        for result in results {
            let error = result.unwrap_err();
            assert_eq!(<&str>::from(error.code()), "CEL_UNKNOWN_IDENTIFIER");
            assert_eq!(error.level(), Level::Info);
            assert!(error.source().unwrap().is::<CelExecutionError>());
            assert!(
                matches!(&error, CelConversionRejection::UnknownIdent { expression, .. } if expression == "missing_variable")
            );
            conversion_contract(error);
        }

        let expr: CelExpression = "42".parse().unwrap();
        let error = expr.try_evaluate::<bool>(&CelContext::new()).unwrap_err();
        assert_eq!(<&str>::from(error.code()), "CEL_BAD_CORE_TYPE_COERCION");
        assert!(matches!(
            &error,
            CelConversionRejection::CoreTypeCoercion(CoreTypeCoercion(
                _,
                CelType::Int,
                CelType::Bool
            ))
        ));
        conversion_contract(error);
    }

    #[test]
    fn json_rejections_cover_recursive_bytes_and_non_string_keys() {
        for source in ["[{'nested': b'x'}]", "{'nested': [{1: 'value'}]}"] {
            let expr: CelExpression = source.parse().unwrap();
            let result: Result<serde_json::Value, CelConversionRejection> =
                expr.try_evaluate(&CelContext::new());
            let error = result.unwrap_err();
            assert_eq!(
                <&str>::from(error.code()),
                match &error {
                    CelConversionRejection::Json(JsonCoercionRejection::UnsupportedBytes {
                        ..
                    }) => {
                        "CEL_JSON_UNSUPPORTED_BYTES"
                    }
                    CelConversionRejection::Json(JsonCoercionRejection::NonStringKey(_)) => {
                        "CEL_JSON_NON_STRING_KEY"
                    }
                    _ => unreachable!(),
                }
            );
            match error {
                CelConversionRejection::Json(JsonCoercionRejection::UnsupportedBytes {
                    expression,
                }) => {
                    assert_eq!(expression, source)
                }
                CelConversionRejection::Json(JsonCoercionRejection::NonStringKey(
                    CoreTypeCoercion(_, CelType::Int, CelType::String),
                )) => {}
                error => panic!("unexpected JSON outcome: {error:?}"),
            }
        }
    }
}
