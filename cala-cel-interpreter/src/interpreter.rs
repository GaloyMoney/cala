use std::sync::Arc;

use cel::Program;
use serde::{Deserialize, Serialize};
use tracing::instrument;

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
        let program =
            Program::compile(&source).map_err(|e| CelError::CelParseError(e.to_string()))?;
        Ok(Self {
            source,
            program: Arc::new(program),
        })
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
