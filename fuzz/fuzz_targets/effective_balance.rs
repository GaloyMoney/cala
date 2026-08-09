#![no_main]

use libfuzzer_sys::fuzz_target;

// Drives EffectiveBalanceData::re_calculate_snapshots (date-partitioned
// balance recomputation). Harness lives in cala_ledger::fuzz. Input: four
// JSON documents concatenated with a 0xFF separator.
fuzz_target!(|data: &[u8]| {
    cala_ledger::fuzz::effective_balance(data);
});
