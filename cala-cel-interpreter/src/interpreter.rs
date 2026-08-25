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
fn compile_program(source: String) -> Result<Arc<Program>, String> {
    let started = std::time::Instant::now();
    let expression = source.clone();
    let result = std::thread::Builder::new()
        .name("cel-compile".to_string())
        .stack_size(COMPILE_STACK_BYTES)
        .spawn(move || {
            Program::compile(&source)
                .map(Arc::new)
                .map_err(|e| e.to_string())
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
            Err(format!("CEL parser panicked during compilation: {msg}"))
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
    pub fn try_evaluate<'a, T: TryFrom<CelResult<'a>, Error = ResultCoercionError>>(
        &'a self,
        ctx: &CelContext,
    ) -> Result<T, CelError> {
        let res = self.evaluate(ctx)?;
        Ok(T::try_from(CelResult {
            expr: &self.source,
            val: res,
        })?)
    }

    #[instrument(name = "cel.evaluate", skip_all, fields(expression = %self.source, context = tracing::field::Empty, result = tracing::field::Empty), err(level = tracing::Level::WARN))]
    pub fn evaluate(&self, ctx: &CelContext) -> Result<CelValue, CelError> {
        let context_debug = ctx.debug_context();
        if !context_debug.is_empty() {
            tracing::Span::current().record("context", &context_debug);
        }

        let value = self
            .program
            .execute(ctx.inner())
            .map_err(|e| CelError::EvaluationError(self.source.clone(), Box::new(e.into())))?;
        let result = CelValue::from_cel_value(value)?;

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
    type Error = CelError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        let program = compile_program(source.clone()).map_err(CelError::CelParseError)?;
        Ok(Self { source, program })
    }
}

impl TryFrom<&str> for CelExpression {
    type Error = CelError;

    fn try_from(source: &str) -> Result<Self, Self::Error> {
        Self::try_from(source.to_string())
    }
}

impl std::str::FromStr for CelExpression {
    type Err = CelError;

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
        assert!(matches!(err, CelError::CelParseError(_)));
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
        assert!(matches!(err, CelError::EvaluationError(_, _)));
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
