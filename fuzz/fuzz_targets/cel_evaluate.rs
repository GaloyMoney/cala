#![no_main]

use cel_interpreter::{CelContext, CelExpression, CelMap};
use libfuzzer_sys::fuzz_target;

// Compile + evaluate arbitrary expressions against a context shaped like
// the ones cala builds in production (tx-template params, velocity
// controls). Errors are expected; panics in builtins (date/decimal/uuid/
// format parsing) or in result coercion are bugs.
fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(expr) = source.parse::<CelExpression>() else {
        return;
    };

    let ctx = production_like_context();

    // Plain evaluation hits the builtins.
    let _ = expr.evaluate(&ctx);

    // Coercion paths hit TryFrom<CelResult> impls in cala-cel-interpreter
    // and cala-ledger-core-types.
    let _ = expr.try_evaluate::<bool>(&ctx);
    let _ = expr.try_evaluate::<String>(&ctx);
    let _ = expr.try_evaluate::<rust_decimal::Decimal>(&ctx);
    let _ = expr.try_evaluate::<uuid::Uuid>(&ctx);
    let _ = expr.try_evaluate::<chrono::NaiveDate>(&ctx);
    let _ = expr.try_evaluate::<chrono::DateTime<chrono::Utc>>(&ctx);
    let _ = expr.try_evaluate::<serde_json::Value>(&ctx);
    let _ = expr.try_evaluate::<cala_types::primitives::Currency>(&ctx);
    let _ = expr.try_evaluate::<cala_types::primitives::Layer>(&ctx);
    let _ = expr.try_evaluate::<cala_types::primitives::DebitOrCredit>(&ctx);
});

fn production_like_context() -> CelContext {
    let mut ctx = CelContext::new();

    // Layer / direction constants (see cala-ledger/src/cel_context.rs).
    ctx.add_variable("SETTLED", "SETTLED");
    ctx.add_variable("PENDING", "PENDING");
    ctx.add_variable("ENCUMBRANCE", "ENCUMBRANCE");
    ctx.add_variable("DEBIT", "DEBIT");
    ctx.add_variable("CREDIT", "CREDIT");

    // tx-template style params.
    let mut params = CelMap::new();
    params.insert("sender", "12e80268-e31c-48bc-9db4-6f96b9aee77a");
    params.insert("recipient", "a5310f2a-08e5-4b3f-9f3a-1b4c6d8e0f12");
    params.insert("amount", "1000.50");
    params.insert("count", 42);
    params.insert("flag", true);
    let mut journal = CelMap::new();
    journal.insert("id", "01936f6a-9f8a-7c3d-8e4b-2a5c7d9e1f30");
    params.insert("journal", journal);
    ctx.add_variable("params", params);

    // velocity-control style variables.
    let mut entry = CelMap::new();
    entry.insert("direction", "DEBIT");
    entry.insert("units", "1.5");
    entry.insert("currency", "USD");
    let mut transaction = CelMap::new();
    transaction.insert("settled", "100.00");
    transaction.insert("pending", "25.5");
    transaction.insert("metadata", serde_json::json!({"foo": {"bar": 1}}));
    entry.insert("transaction", transaction);
    ctx.add_variable("entry", entry);

    ctx.add_variable(
        "time",
        chrono::NaiveDate::from_ymd_opt(2024, 6, 1)
            .unwrap()
            .and_hms_opt(12, 30, 0)
            .unwrap()
            .and_utc(),
    );

    ctx
}
