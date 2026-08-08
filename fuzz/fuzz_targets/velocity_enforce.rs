#![no_main]

use cala_ledger::{
    es_entity::clock::Clock,
    velocity::{AccountVelocityControl, EvalContext},
};
use cala_types::{
    balance::BalanceSnapshot,
    entry::EntryValues,
    transaction::TransactionValues,
    velocity::VelocityContextAccountValues,
};
use libfuzzer_sys::fuzz_target;

// Drives the pure velocity-enforcement logic end to end:
//   needs_enforcement -> window_for_enforcement -> enforce
// against a fuzzed control, entry, balance snapshot, transaction and account.
// This is where financial limits are actually compared, so it spans CEL
// evaluation, decimal arithmetic and time-window logic.
//
// The input is five JSON documents concatenated with a 0xFF separator. 0xFF is
// never valid inside JSON/UTF-8, so the split is unambiguous and lets the
// fuzzer mutate each document independently.
fuzz_target!(|data: &[u8]| {
    let parts: Vec<&[u8]> = data.split(|&b| b == 0xFF).collect();
    if parts.len() < 5 {
        return;
    }
    let Ok(control) = serde_json::from_slice::<AccountVelocityControl>(parts[0]) else {
        return;
    };
    let Ok(entry) = serde_json::from_slice::<EntryValues>(parts[1]) else {
        return;
    };
    let Ok(snapshot) = serde_json::from_slice::<BalanceSnapshot>(parts[2]) else {
        return;
    };
    let Ok(tx) = serde_json::from_slice::<TransactionValues>(parts[3]) else {
        return;
    };
    let Ok(account) = serde_json::from_slice::<VelocityContextAccountValues>(parts[4]) else {
        return;
    };

    // The account must be registered before we ask for its entry context
    // (context_for_entry expects it, panicking otherwise).
    let account_id = account.id;
    let mut eval = EvalContext::new(Clock::handle().clone(), &tx, std::iter::once(&account));
    let ctx = eval.context_for_entry(account_id, &entry);

    // Control-level condition.
    let _ = control.needs_enforcement(&ctx);

    for limit in &control.velocity_limits {
        let _ = limit.window_for_enforcement(&ctx, &entry);
        let _ = limit.enforce(&ctx, snapshot.created_at, &snapshot);
    }
});
