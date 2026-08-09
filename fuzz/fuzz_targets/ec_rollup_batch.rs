#![no_main]

use libfuzzer_sys::fuzz_target;

// Drives EcRollupBatch (the pure accumulator behind the streaming EC-balance
// rollup). Harness lives in cala_ledger::fuzz. Input: two JSON documents
// (txns, entries) concatenated with a 0xFF separator.
fuzz_target!(|data: &[u8]| {
    cala_ledger::fuzz::ec_rollup_batch(data);
});
