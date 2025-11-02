use tracing::instrument;

use crate::{cel_type::*, error::*, value::*};

use super::assert_arg;

#[instrument(name = "cel.builtin.timestamp.cast", skip_all, err, level = "debug")]
pub fn cast(args: Vec<CelValue>) -> Result<CelValue, CelError> {
    match args.first() {
        Some(CelValue::String(s)) => Ok(CelValue::Timestamp(
            s.parse()
                .map_err(|e| CelError::TimestampError(format!("{e:?}")))?,
        )),
        Some(v) => Err(CelError::BadType(CelType::Timestamp, CelType::from(v))),
        None => Err(CelError::MissingArgument),
    }
}

#[instrument(name = "cel.builtin.timestamp.format", skip_all, err, level = "debug")]
pub fn format(target: &CelValue, args: Vec<CelValue>) -> Result<CelValue, CelError> {
    if let CelValue::Timestamp(ts) = target {
        let format: std::sync::Arc<String> = assert_arg(args.first())?;
        Ok(CelValue::String(ts.format(&format).to_string().into()))
    } else {
        Err(CelError::BadType(CelType::Timestamp, CelType::from(target)))
    }
}
