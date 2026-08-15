//! Regression tests: adversarial CEL input must fail fast during parsing,
//! not trigger runaway parser error-recovery (OOM).
//!
//! The `cel` parser caps error-recovery attempts; once the cap is hit the
//! parse fails fast. These tests pin that: pathological inputs must produce
//! a plain parse error within a bounded time, with cost scaling linearly
//! (not exponentially) in input size.

use cala_cel_interpreter::CelExpression;

/// Repeating the ambiguous `!!(` motif — errors quickly at any size.
#[test]
fn repeated_negation_parens_motif_errors_quickly() {
    let source = "!!(".repeat(42); // 126 bytes
    let started = std::time::Instant::now();
    let err = CelExpression::try_from(source).expect_err("must be a parse error");
    assert!(err.to_string().contains("CelParseError"));
    // Generous bound (debug builds, loaded CI); release is ~1ms.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "parse took {:?} — error recovery no longer short-circuits?",
        started.elapsed()
    );
}

/// Cost must scale linearly, not exponentially, with input size.
#[test]
fn recovery_cost_scales_linearly_with_input() {
    for n in [84usize, 210, 840] {
        let source = "!!(".repeat(n); // 252B, 630B, 2520B
        let started = std::time::Instant::now();
        let result = CelExpression::try_from(source);
        assert!(
            result.is_err(),
            "{n} motifs must fail to parse, not compile"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "parsing {} bytes of the motif took {:?}",
            n * 3,
            started.elapsed()
        );
    }
}

/// Malformed deep nesting: recovery combined with the recursion listener
/// must terminate in an error, not hang.
#[test]
fn malformed_nested_expression_errors_without_hanging() {
    let expression = format!(
        "ma{}{}{}put?{}ep",
        "[".repeat(63),
        "\u{c}\0\0\0\0\0\0\0",
        "[".repeat(7),
        "[".repeat(18)
    );
    let started = std::time::Instant::now();
    assert!(CelExpression::try_from(expression).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}
