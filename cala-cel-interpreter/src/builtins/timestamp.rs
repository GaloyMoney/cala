use crate::{cel_type::*, error::*, value::*};

use super::assert_arg;

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

pub fn format(target: &CelValue, args: Vec<CelValue>) -> Result<CelValue, CelError> {
    if let CelValue::Timestamp(ts) = target {
        let format: std::sync::Arc<String> = assert_arg(args.first())?;
        Ok(CelValue::String(ts.format(&format).to_string().into()))
    } else {
        Err(CelError::BadType(CelType::Timestamp, CelType::from(target)))
    }
}
