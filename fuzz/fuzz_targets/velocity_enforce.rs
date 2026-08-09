#![no_main]

use libfuzzer_sys::fuzz_target;

// Drives the pure velocity-enforcement logic (needs_enforcement ->
// window_for_enforcement -> enforce). The harness lives inside cala-ledger
// (`cala_ledger::fuzz`) so it can reach pub(crate) types; this target just
// feeds it bytes. Input: five JSON documents concatenated with a 0xFF
// separator (see fuzz/seeds/velocity_enforce/).
fuzz_target!(|data: &[u8]| {
    cala_ledger::fuzz::velocity_enforce(data);
});
