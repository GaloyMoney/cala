#![no_main]

use cala_types::{
    account::AccountValues,
    account_set::AccountSetValues,
    balance::{BalanceSnapshot, EffectiveBalanceSnapshot},
    entry::EntryValues,
    journal::JournalValues,
    outbox::OutboxEventPayload,
    param::ParamDefinition,
    primitives::{Currency, Layer},
    transaction::TransactionValues,
    tx_template::TxTemplateValues,
    velocity::{VelocityControlValues, VelocityLimitValues},
};
use libfuzzer_sys::fuzz_target;

// Core types are (de)serialized from the DB and from API input. Arbitrary
// JSON must produce errors, never panics. Several fields use
// `#[serde(try_from = "String")]`, so this also drives CEL compilation
// through the serde path.
fuzz_target!(|data: &[u8]| {
    macro_rules! roundtrip {
        ($ty:ty) => {
            if let Ok(v) = serde_json::from_slice::<$ty>(data) {
                let _ = serde_json::to_string(&v);
            }
        };
    }

    roundtrip!(ParamDefinition);
    roundtrip!(VelocityLimitValues);
    roundtrip!(VelocityControlValues);
    roundtrip!(TxTemplateValues);
    roundtrip!(TransactionValues);
    roundtrip!(EntryValues);
    roundtrip!(AccountValues);
    roundtrip!(AccountSetValues);
    roundtrip!(JournalValues);
    roundtrip!(BalanceSnapshot);
    roundtrip!(EffectiveBalanceSnapshot);
    roundtrip!(OutboxEventPayload);
    roundtrip!(Layer);

    // Hand-rolled string parsers.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = s.parse::<Currency>();
    }
});
