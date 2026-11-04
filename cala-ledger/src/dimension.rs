use cel_interpreter::{CelExpression, CelParseRejection};
use tracing::instrument;

#[derive(Clone, Debug)]
pub struct NewPartitionKey {
    pub(crate) alias: String,
    pub(crate) value: String,
}

#[bon::bon]
impl NewPartitionKey {
    #[builder]
    pub fn new(
        #[builder(into)] alias: String,
        #[builder(into)] value: String,
    ) -> Result<Self, CelParseRejection> {
        validate_expression(&value)?;
        Ok(Self { alias, value })
    }
}

#[instrument(name = "velocity_limit.validate_expression", skip(expr), fields(expression = %expr), err(level = tracing::Level::WARN))]
fn validate_expression(expr: &str) -> Result<(), CelParseRejection> {
    CelExpression::try_from(expr)?;
    Ok(())
}
