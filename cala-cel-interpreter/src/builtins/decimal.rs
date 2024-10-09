use rust_decimal::Decimal;

use crate::{cel_type::*, error::*, value::*};

use super::assert_arg;

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
