#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use cala_types::param::ParamDataType;
use cel_interpreter::{CelArray, CelMap, CelValue};
use libfuzzer_sys::fuzz_target;
use std::sync::Arc;

// `ParamDataType::coerce_value` takes a CelValue and turns it into the
// requested type (with string -> Uuid/Decimal/Date parsing etc.). It must
// never panic on any CelValue. CelValue has no Deserialize impl, so we build
// structurally-arbitrary values from the fuzz input by hand, then run every
// coercion.
fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Some(value) = arb_value(&mut u, 0) else {
        return;
    };

    for ty in [
        ParamDataType::String,
        ParamDataType::Integer,
        ParamDataType::Decimal,
        ParamDataType::Boolean,
        ParamDataType::Uuid,
        ParamDataType::Date,
        ParamDataType::Timestamp,
        ParamDataType::Json,
    ] {
        let _ = ty.coerce_value(value.clone());
    }
});

fn arb_value(u: &mut Unstructured, depth: u8) -> Option<CelValue> {
    // Bias towards the cheap scalars that drive the interesting parsing paths.
    let tag: u8 = u.int_in_range(0..=11).ok()?;
    Some(match tag {
        0 => CelValue::Null,
        1 => CelValue::Bool(bool::arbitrary(u).ok()?),
        2 => CelValue::Int(i64::arbitrary(u).ok()?),
        3 => CelValue::UInt(u64::arbitrary(u).ok()?),
        4 => CelValue::Double(f64::arbitrary(u).ok()?),
        5 => {
            // Arbitrary bytes reinterpreted as a string. This is the path that
            // reaches the Uuid/Decimal/Date string parsers, which are the most
            // likely to misbehave.
            let bytes = <Vec<u8>>::arbitrary(u).ok()?;
            CelValue::String(String::from_utf8_lossy(&bytes).into_owned().into())
        }
        6 => CelValue::Bytes(Arc::new(<Vec<u8>>::arbitrary(u).ok()?)),
        7 => {
            // Decimal from an arbitrary i128 mantissa + scale so we exercise a
            // wide range without relying on Decimal: Arbitrary.
            let mantissa = i64::arbitrary(u).ok()?;
            let scale = u.int_in_range(0..=27u32).ok()?;
            rust_decimal::Decimal::try_from_i128_with_scale(mantissa as i128, scale)
                .unwrap_or_default()
                .into()
        }
        8 => {
            // Date from an arbitrary day count since the epoch.
            let days = i32::arbitrary(u).ok()?;
            chrono::NaiveDate::from_num_days_from_ce_opt(days)
                .map(CelValue::Date)
                .unwrap_or(CelValue::Null)
        }
        9 => {
            let days = i32::arbitrary(u).ok()?;
            let secs = u.int_in_range(0..=86_400u32).ok()?;
            let ts = chrono::NaiveDate::from_num_days_from_ce_opt(days)
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
                        + chrono::Duration::seconds(secs as i64)
                });
            ts.map(CelValue::Timestamp).unwrap_or(CelValue::Null)
        }
        10 => {
            // Uuid from 16 arbitrary bytes.
            let mut buf = [0u8; 16];
            if u.fill_buffer(&mut buf).is_err() {
                return None;
            }
            CelValue::Uuid(uuid::Uuid::from_bytes(buf))
        }
        // Containers: bounded depth to keep the value finite and fast.
        _ if depth < 4 => {
            let tag: u8 = u.int_in_range(0..=1).ok()?;
            match tag {
                0 => {
                    let n = u.int_in_range(0..=4u8).ok()?;
                    let mut map = CelMap::new();
                    for _ in 0..n {
                        let k: String = String::arbitrary(u).ok()?;
                        if let Some(v) = arb_value(u, depth + 1) {
                            map.insert(k, v);
                        }
                    }
                    map.into()
                }
                _ => {
                    let n = u.int_in_range(0..=4u8).ok()?;
                    let mut list = CelArray::new();
                    for _ in 0..n {
                        if let Some(v) = arb_value(u, depth + 1) {
                            list.push(v);
                        }
                    }
                    CelValue::List(Arc::new(list))
                }
            }
        }
        _ => CelValue::Null,
    })
}
