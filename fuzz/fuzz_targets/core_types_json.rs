#![no_main]

use cala_types::{
    entry::EntryValues,
    param::ParamDefinition,
    primitives::{Currency, Layer},
    tx_template::TxTemplateValues,
    velocity::VelocityLimitValues,
};
use libfuzzer_sys::fuzz_target;

// Core types are (de)serialized from the DB and from API input. Arbitrary
// JSON must produce errors, never panics — the `CelExpression` fields use
// `#[serde(try_from = "String")]`, so this also drives CEL compilation
// through the serde path.
fuzz_target!(|data: &[u8]| {
    // Deserialization of user/DB-controlled shapes.
    if let Ok(v) = serde_json::from_slice::<ParamDefinition>(data) {
        let _ = serde_json::to_string(&v);
    }
    if let Ok(v) = serde_json::from_slice::<VelocityLimitValues>(data) {
        let _ = serde_json::to_string(&v);
    }
    if let Ok(v) = serde_json::from_slice::<TxTemplateValues>(data) {
        let _ = serde_json::to_string(&v);
    }
    if let Ok(v) = serde_json::from_slice::<EntryValues>(data) {
        let _ = serde_json::to_string(&v);
    }
    if let Ok(v) = serde_json::from_slice::<Layer>(data) {
        let _ = serde_json::to_string(&v);
    }

    // Hand-rolled string parsers.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = s.parse::<Currency>();
    }
});
