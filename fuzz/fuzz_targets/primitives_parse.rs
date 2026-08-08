#![no_main]

use cala_ledger::balance::{AccountBalanceByCurrencyCursor, AccountBalanceCursor};
use cala_types::primitives::{Currency, DebitOrCredit};
use libfuzzer_sys::fuzz_target;

// Cheap target for the hand-rolled / derived string parsers and the
// pagination cursors: arbitrary input must produce errors, never panics.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = s.parse::<DebitOrCredit>();
        let _ = s.parse::<Currency>();
    }

    if let Ok(c) = serde_json::from_slice::<AccountBalanceCursor>(data) {
        let _ = serde_json::to_string(&c);
    }
    if let Ok(c) = serde_json::from_slice::<AccountBalanceByCurrencyCursor>(data) {
        let _ = serde_json::to_string(&c);
    }
});
