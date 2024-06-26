use chrono::{NaiveDate, Utc};

use std::sync::Arc;

use super::{cel_type::*, value::*};
use crate::error::*;

pub(crate) fn date(args: Vec<CelValue>) -> Result<CelValue, CelError> {
    if args.is_empty() {
        return Ok(CelValue::Date(Utc::now().date_naive()));
    }

    let s: Arc<String> = assert_arg(args.first())?;
    Ok(CelValue::Date(NaiveDate::parse_from_str(&s, "%Y-%m-%d")?))
}

pub(crate) fn uuid(args: Vec<CelValue>) -> Result<CelValue, CelError> {
    let s: Arc<String> = assert_arg(args.first())?;
    Ok(CelValue::Uuid(
        s.parse()
            .map_err(|e| CelError::UuidError(format!("{e:?}")))?,
    ))
}

pub(crate) mod decimal {
    use rust_decimal::Decimal;

    use super::*;

    pub fn cast(args: Vec<CelValue>) -> Result<CelValue, CelError> {
        match args.first() {
            Some(CelValue::Decimal(d)) => Ok(CelValue::Decimal(*d)),
            Some(CelValue::String(s)) => Ok(CelValue::Decimal(
                s.parse()
                    .map_err(|e| CelError::DecimalError(format!("{e:?}")))?,
            )),
            Some(v) => Err(CelError::BadType(CelType::Decimal, CelType::from(v))),
            None => Err(CelError::MissingArgument),
        }
    }

    pub fn add(args: Vec<CelValue>) -> Result<CelValue, CelError> {
        let a: &Decimal = assert_arg(args.first())?;
        let b: &Decimal = assert_arg(args.get(1))?;
        Ok(CelValue::Decimal(a + b))
    }
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
