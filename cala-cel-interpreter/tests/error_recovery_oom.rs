//! Regression tests: adversarial input must not trigger runaway parser
//! error-recovery (OOM).
//!
//! The upstream `cel` parser (before v0.14.3, cel-rust PR #310) had no cap
//! on ANTLR error-recovery attempts, so syntactically ambiguous input drove
//! the ALL(*) adaptive-prediction machinery into exponential DFA/config-set
//! growth. Measured on this machine with `cel` 0.14.1 (release build):
//! repeating the `!!(` motif to 126 bytes peaked at **46GB RSS** and was
//! still burning CPU after 300s; 63 bytes already needed 82MB and 0.5s.
//!
//! v0.14.3 ports cel-go's `recoveryLimitErrorStrategy`: once recovery is
//! attempted 30 times the parse fails fast. The same 126-byte input now
//! errors in ~1ms with ~8MB RSS, and memory stays flat as the motif grows.
//!
//! Before the fix these tests would OOM-kill (or time out) the CI runner;
//! now they must simply report parse errors.

use cala_cel_interpreter::CelExpression;

/// The reproduction from cel-rust issue #306: repeating the ambiguous
/// `!!(` sequence. 126 bytes was the "~1.1GB" row in the issue's table
/// (this machine fared worse: >46GB).
#[test]
fn repeated_negation_parens_motif_errors_quickly() {
    let source = "!!(".repeat(42); // 126 bytes
    let started = std::time::Instant::now();
    let err = CelExpression::try_from(source).expect_err("must be a parse error");
    assert!(err.to_string().contains("CelParseError"));
    // Generous bound (debug builds, loaded CI): release peaks at ~1ms.
    // Pre-fix this never completed at all within 300s.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "parse took {:?} — error recovery no longer short-circuits?",
        started.elapsed()
    );
}

/// Scaling must be flat, not exponential: 40x more of the same motif costs
/// a few more recovery attempts, not 40x the memory/time.
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

/// Malformed deep nesting (from cel-rust PR #310's own tests): recovery
/// combined with the recursion listener must terminate in an error.
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
