pub(crate) mod decimal;
pub(crate) mod timestamp;

use chrono::NaiveDate;
use tracing::instrument;

use std::sync::Arc;

use super::value::*;
use crate::context::CelContext;
use crate::error::*;

#[instrument(name = "cel.builtin.date", skip_all, level = "debug", err)]
pub(crate) fn date(ctx: &CelContext, args: Vec<CelValue>) -> Result<CelValue, CelError> {
    if args.is_empty() {
        return Ok(CelValue::Date(ctx.clock().now().date_naive()));
    }

    let s: Arc<String> = assert_arg(args.first())?;
    Ok(CelValue::Date(NaiveDate::parse_from_str(&s, "%Y-%m-%d")?))
}

#[instrument(name = "cel.builtin.uuid", skip_all, level = "debug", err)]
pub(crate) fn uuid(args: Vec<CelValue>) -> Result<CelValue, CelError> {
    let s: Arc<String> = assert_arg(args.first())?;
    Ok(CelValue::Uuid(
        s.parse()
            .map_err(|e| CelError::UuidError(format!("{e:?}")))?,
    ))
}

fn assert_arg<'a, T: TryFrom<&'a CelValue, Error = CelError>>(
    arg: Option<&'a CelValue>,
) -> Result<T, CelError> {
    if let Some(v) = arg {
        T::try_from(v)
    } else {
        Err(CelError::MissingArgument)
    }
}
