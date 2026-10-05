use super::primitives::ParseCurrencyError;
use cel_interpreter::*;
use es_entity::errlanes;

#[derive(Debug, errlanes::Rejection)]
pub enum CurrencyCoercionRejection {
    #[error("{0}")]
    #[rejection(delegate, from)]
    ExternalTypeCoercion(ExternalTypeCoercion),
    #[rejection(code = "CEL_INVALID_CURRENCY")]
    #[error("Invalid currency in '{}': {}", expression, source)]
    InvalidCurrency {
        expression: String,
        #[source]
        source: ParseCurrencyError,
    },
}

impl From<CurrencyCoercionRejection> for CelConversionRejection {
    fn from(error: CurrencyCoercionRejection) -> Self {
        match error {
            CurrencyCoercionRejection::ExternalTypeCoercion(source) => source.into(),
            CurrencyCoercionRejection::InvalidCurrency { expression, source } => {
                Self::ExternalParse(ExternalParseError {
                    expression,
                    type_name: "currency",
                    source: Box::new(source),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Currency, DebitOrCredit, Layer};
    use es_entity::errlanes::{Level, Rejection};
    use std::error::Error;

    #[test]
    fn every_target_returns_the_same_evaluation_failure() {
        let context = CelContext::new();
        let expression: CelExpression = "missing_variable".parse().unwrap();
        let results: [Result<(), CelConversionRejection>; 5] = [
            expression.try_evaluate::<bool>(&context).map(|_| ()),
            expression.try_evaluate::<Layer>(&context).map(|_| ()),
            expression
                .try_evaluate::<DebitOrCredit>(&context)
                .map(|_| ()),
            expression.try_evaluate::<Currency>(&context).map(|_| ()),
            expression
                .try_evaluate::<serde_json::Value>(&context)
                .map(|_| ()),
        ];
        for result in results {
            let error = result.unwrap_err();
            assert_eq!(<&str>::from(error.code()), "CEL_EVALUATION_ERROR");
            assert_eq!(error.level(), Level::Info);
            assert!(error.source().unwrap().is::<CelExecutionError>());
            assert!(matches!(
                error,
                CelConversionRejection::UnknownIdent { expression, .. }
                    if expression == "missing_variable"
            ));
        }
    }

    #[test]
    fn external_targets_share_conversion_contract_and_preserve_sources() {
        let context = CelContext::new();
        let expression: CelExpression = "'INVALID'".parse().unwrap();
        let currency: Result<Currency, CelConversionRejection> = expression.try_evaluate(&context);
        let error = currency.unwrap_err();
        assert_eq!(<&str>::from(error.code()), "CEL_RESULT_COERCION_ERROR");
        assert_eq!(error.level(), Level::Info);
        assert!(error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<ParseCurrencyError>());
        assert!(matches!(
            &error,
            CelConversionRejection::ExternalParse(ExternalParseError {
                type_name: "currency",
                ..
            })
        ));
        let direct = Currency::try_from(CelResult {
            expr: "'INVALID'",
            val: CelValue::from("INVALID"),
        })
        .unwrap_err();
        assert_eq!(error.to_string(), direct.to_string());
        let layer: Result<Layer, CelConversionRejection> = expression.try_evaluate(&context);
        assert!(matches!(
            layer,
            Err(CelConversionRejection::ExternalTypeCoercion(_))
        ));
    }
}
