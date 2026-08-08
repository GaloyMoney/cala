//! Regression tests: CEL compilation must not depend on the caller's stack.
//!
//! The `cel` crate parses with an ANTLR-generated recursive-descent parser
//! whose debug-build frames are enormous: ~350KiB of stack for a trivial
//! two-operator expression and >8MiB for expressions at its grammar-recursion
//! cap (96). When compilation ran on the caller's thread it would overflow
//! whenever the caller sat on top of deep async state machines (stack
//! overflows in downstream debug tests — see GaloyMoney/lana-bank#8011, which
//! papered over it with RUST_MIN_STACK=8388608).
//!
//! `CelExpression` now compiles on a dedicated thread with a known-large
//! stack. These tests construct expressions from threads with tiny stacks —
//! far smaller than the parser alone needs — to pin that independence: before
//! the fix every one of them dies with SIGABRT (stack overflow).

use cala_cel_interpreter::{CelContext, CelExpression, CelValue};

const SMALL_CALLER_STACK: usize = 128 * 1024;

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(SMALL_CALLER_STACK)
        .spawn(f)
        .expect("spawn small-stack thread")
        .join()
        .expect("small-stack thread panicked")
}

#[test]
fn compiles_realistic_velocity_expression_on_small_caller_stack() {
    // A flat velocity-limit style condition: needs ~350KiB to parse in debug,
    // i.e. it cannot possibly have parsed on this 128KiB thread.
    on_small_stack(|| {
        CelExpression::try_from(
            "context.vars.entry.units > decimal('100') && context.vars.entry.currency == 'USD'",
        )
        .expect("realistic velocity expression must compile");
    });
}

#[test]
fn compiles_and_evaluates_deeply_nested_expression_on_small_caller_stack() {
    // Depth 64 is accepted by cel's grammar-recursion cap (96) but needs
    // >4MiB of parser stack in debug builds — more than lana-bank's 8MiB
    // RUST_MIN_STACK workaround could guarantee as headroom.
    on_small_stack(|| {
        let source = format!("{}1{}", "(".repeat(64), ")".repeat(64));
        let expression = CelExpression::try_from(source).expect("depth-64 nesting must compile");
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).expect("evaluate"),
            CelValue::Int(1)
        );
    });
}

#[test]
fn nesting_beyond_recursion_cap_errors_without_crashing() {
    // Reaching the cap requires the parser to recurse to depth 96 first
    // (>8MiB of stack in debug) before it reports the error — the caller's
    // stack must not be what decides between Err and SIGABRT.
    on_small_stack(|| {
        let source = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        assert!(CelExpression::try_from(source).is_err());
    });
}
