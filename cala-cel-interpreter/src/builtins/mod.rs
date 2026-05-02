use std::sync::Arc;

use cel::{
    extractors::{Arguments, This},
    objects::Value,
    ExecutionError,
};
use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};
use es_entity::clock::ClockHandle;

use crate::value::{CelDecimal, CelUuid};

type Result<T> = std::result::Result<T, ExecutionError>;

pub(crate) fn date(clock: ClockHandle, Arguments(args): Arguments) -> Result<Value> {
    let date = match args.as_slice() {
        [] => clock.now().date_naive(),
        [Value::String(s)] => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| ExecutionError::function_error("date", e))?,
        [Value::Timestamp(ts)] => ts.date_naive(),
        [v] => {
            return Err(ExecutionError::function_error(
                "date",
                format!("cannot convert {v:?} to date"),
            ))
        }
        values => {
            return Err(ExecutionError::invalid_argument_count(1, values.len()));
        }
    };

    let dt = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    Ok(Value::Timestamp(
        FixedOffset::east_opt(0)
            .expect("UTC offset is valid")
            .from_utc_datetime(&dt),
    ))
}

pub(crate) fn uuid(Arguments(args): Arguments) -> Result<Value> {
    match args.as_slice() {
        [Value::String(s)] => {
            let id = s
                .parse()
                .map_err(|e| ExecutionError::function_error("uuid", format!("{e:?}")))?;
            Ok(Value::Opaque(Arc::new(CelUuid(id))))
        }
        [v] => Err(ExecutionError::function_error(
            "uuid",
            format!("cannot convert {v:?} to uuid"),
        )),
        values => Err(ExecutionError::invalid_argument_count(1, values.len())),
    }
}

pub(crate) fn decimal(Arguments(args): Arguments) -> Result<Value> {
    match args.as_slice() {
        [Value::Opaque(o)] if o.runtime_type_name() == "cala.Decimal" => {
            Ok(Value::Opaque(o.clone()))
        }
        [Value::String(s)] => {
            let decimal = s
                .parse()
                .map_err(|e| ExecutionError::function_error("decimal", format!("{e:?}")))?;
            Ok(Value::Opaque(Arc::new(CelDecimal(decimal))))
        }
        [Value::Int(i)] => Ok(Value::Opaque(Arc::new(CelDecimal((*i).into())))),
        [Value::UInt(u)] => Ok(Value::Opaque(Arc::new(CelDecimal((*u).into())))),
        [v] => Err(ExecutionError::function_error(
            "decimal",
            format!("cannot convert {v:?} to decimal"),
        )),
        values => Err(ExecutionError::invalid_argument_count(1, values.len())),
    }
}

pub(crate) fn decimal_add(Arguments(args): Arguments) -> Result<Value> {
    match args.as_slice() {
        [left, right] => {
            let left = decimal_from_value(left)?;
            let right = decimal_from_value(right)?;
            Ok(Value::Opaque(Arc::new(CelDecimal(left + right))))
        }
        values => Err(ExecutionError::invalid_argument_count(2, values.len())),
    }
}

pub(crate) fn timestamp_format(This(this): This<Value>, format: Arc<String>) -> Result<Value> {
    match this {
        Value::Timestamp(ts) => Ok(Value::String(
            ts.with_timezone(&Utc).format(&format).to_string().into(),
        )),
        v => Err(ExecutionError::function_error(
            "format",
            format!("cannot format {v:?} as timestamp"),
        )),
    }
}

fn decimal_from_value(value: &Value) -> Result<rust_decimal::Decimal> {
    match value {
        Value::Opaque(o) if o.runtime_type_name() == "cala.Decimal" => {
            let decimal = o.downcast_ref::<CelDecimal>().ok_or_else(|| {
                ExecutionError::function_error("decimal", "failed to downcast decimal")
            })?;
            Ok(decimal.0)
        }
        Value::String(s) => s
            .parse()
            .map_err(|e| ExecutionError::function_error("decimal", format!("{e:?}"))),
        Value::Int(i) => Ok((*i).into()),
        Value::UInt(u) => Ok((*u).into()),
        v => Err(ExecutionError::function_error(
            "decimal",
            format!("cannot convert {v:?} to decimal"),
        )),
    }
}
