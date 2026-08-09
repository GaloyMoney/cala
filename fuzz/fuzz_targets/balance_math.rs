#![no_main]

use cala_ledger::balance::AccountBalance;
use cala_types::{
    balance::BalanceSnapshot,
    primitives::{DebitOrCredit, Layer},
};
use libfuzzer_sys::fuzz_target;

// AccountBalance derives layer/direction math (settled - pending +/- encumbrance)
// straight from BalanceSnapshot. Both come from the DB and from API input, so
// arbitrary decimals must never panic the accessors. This is the surface that
// PR #804 ("harden arithmetic against overflow") targets; fuzzing validates it
// and keeps it regression-free.
fuzz_target!(|data: &[u8]| {
    let Ok(snapshot) = serde_json::from_slice::<BalanceSnapshot>(data) else {
        return;
    };

    for direction in [DebitOrCredit::Debit, DebitOrCredit::Credit] {
        let bal = AccountBalance {
            balance_type: direction,
            details: snapshot.clone(),
        };
        let _ = bal.settled();
        let _ = bal.pending();
        let _ = bal.encumbrance();
        for layer in [Layer::Settled, Layer::Pending, Layer::Encumbrance] {
            let _ = bal.available(layer);
        }
    }

    // BalanceSnapshot::available / rollup does unchecked `+` across layers;
    // that's where Decimal overflow lives.
    for layer in [Layer::Settled, Layer::Pending, Layer::Encumbrance] {
        let _ = snapshot.available(layer);
    }

    // The direction may also arrive over the wire; deserialize it too.
    if let Ok(d) = serde_json::from_slice::<DebitOrCredit>(data) {
        let bal = AccountBalance {
            balance_type: d,
            details: snapshot,
        };
        let _ = bal.available(Layer::Settled);
    }
});
