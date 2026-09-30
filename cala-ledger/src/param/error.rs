use thiserror::Error;

use cel_interpreter::CelError;

/// Params carries no infrastructure failures of its own — evaluating a CEL
/// expression against caller-supplied params is a pure computation — so
/// there is no `ParamError` wrapper: every failure here is a domain
/// rejection a caller can act on (fix the input and retry), and modules
/// that embed params (`tx_template`, `velocity`) lift it directly into
/// their own rejection enum.
// Not `Clone`: `CelError` (from `cala-cel-interpreter`, out of this
// rollout's scope) does not implement it.
#[derive(Debug, Error, errlanes::Rejection)]
pub enum ParamRejection {
    #[error("ParamError - ParamTypeMismatch: {0}")]
    ParamTypeMismatch(String),
    #[error("ParamError - CelError: {0}")]
    CelError(#[from] CelError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tx_template::error::TxTemplateEvaluationRejection, velocity::error::VelocityRejection,
    };
    use errlanes::Rejection;

    #[test]
    fn explicit_lifts_preserve_unprefixed_param_cases() {
        let source = ParamRejection::ParamTypeMismatch("amount".into());
        let code: &'static str = source.code().into();
        let level = source.level();
        let message = source.to_string();
        let template = TxTemplateEvaluationRejection::from(source);
        let velocity = VelocityRejection::from(ParamRejection::ParamTypeMismatch("amount".into()));

        assert!(
            matches!(&template, TxTemplateEvaluationRejection::ParamTypeMismatch(p) if p == "amount")
        );
        assert!(matches!(&velocity, VelocityRejection::ParamTypeMismatch(p) if p == "amount"));
        assert_eq!(Into::<&'static str>::into(template.code()), code);
        assert_eq!(Into::<&'static str>::into(velocity.code()), code);
        assert_eq!(template.level(), level);
        assert_eq!(velocity.level(), level);
        assert_eq!(template.to_string(), message);
        assert_eq!(velocity.to_string(), message);
    }
}
