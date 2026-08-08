#![no_main]

use cel_interpreter::CelExpression;
use libfuzzer_sys::fuzz_target;

// Arbitrary strings must never crash CEL compilation — errors are fine,
// panics / stack overflows / hangs are not.
// Compilation happens on a dedicated thread with a large stack (see
// `compile_program` in cala-cel-interpreter), so parser recursion
// depth is contained by design; this target verifies that holds for
// adversarial input.
fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let _ = source.parse::<CelExpression>();
});
