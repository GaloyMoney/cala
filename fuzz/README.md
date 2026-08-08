# Fuzz testing for cala

Coverage-guided fuzzing with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
(libFuzzer). Targets the pure, in-memory attack surface of the library:
CEL expression compilation/evaluation and serde (de)serialization of the
core types that come from API input and the DB.

## Targets

| Target             | What it exercises |
| ------------------ | ----------------- |
| `cel_compile`      | `CelExpression::try_from` on arbitrary strings — the upstream `cel` ANTLR parser, cala's dedicated big-stack compile thread and the compilation cache. |
| `cel_evaluate`     | Compile + evaluate against a production-like context (tx-template `params`, velocity `entry`/`time`, layer/direction constants), plus all `TryFrom<CelResult>` coercions (`bool`, `String`, `Decimal`, `Uuid`, dates, `serde_json::Value`, `Currency`, `Layer`, `DebitOrCredit`). Hits the `date`/`uuid`/`decimal.*`/`format` builtins. |
| `core_types_json`  | `serde_json` round-trips of every core `*Values`/snapshot type (`AccountValues`, `AccountSetValues`, `JournalValues`, `TransactionValues`, `EntryValues`, `BalanceSnapshot`, `EffectiveBalanceSnapshot`, `VelocityControlValues`, `VelocityLimitValues`, `TxTemplateValues`, `ParamDefinition`, `OutboxEventPayload`, `Layer`) and the hand-rolled `Currency` string parser. CEL fields use `#[serde(try_from = "String")]`, so this also drives compilation through the serde path. |
| `param_coerce`     | Builds structurally-arbitrary `CelValue`s from the fuzz input by hand (scalars, nested maps/lists, and lossy-UTF8 strings) and runs `ParamDataType::coerce_value` for all eight types — exercises the `String`→`Uuid`/`Decimal`/`Date` parsers with adversarial values. |
| `balance_math`     | Deserializes `BalanceSnapshot` (incl. decimals near `Decimal::MAX`) and drives `BalanceSnapshot::available`/`rollup` and `AccountBalance`'s `settled`/`pending`/`encumbrance`/`available` accessors. Validates the arithmetic-overflow hardening surface. |

## Running

Requires `cargo-fuzz` (`cargo install cargo-fuzz`). On macOS / stable Rust
use `-s none` (the default AddressSanitizer build requires a nightly
toolchain):

```sh
# one-time: bootstrap the corpus from the committed seeds
for t in cel_compile cel_evaluate core_types_json param_coerce balance_math; do
  mkdir -p fuzz/corpus/$t && cp fuzz/seeds/$t/* fuzz/corpus/$t/
done

# fuzz (Ctrl-C to stop)
cargo fuzz run -s none cel_compile
cargo fuzz run -s none cel_evaluate
cargo fuzz run -s none core_types_json
cargo fuzz run -s none param_coerce
cargo fuzz run -s none balance_math
```

`balance_math` depends on `cala-ledger`, which compiles without a database
via the committed SQLx offline cache but needs `SQLX_OFFLINE=true` in the
environment:

```sh
SQLX_OFFLINE=true cargo fuzz run -s none balance_math
```

`fuzz/corpus/` is gitignored — it is the working corpus the fuzzer grows.
`fuzz/seeds/` is committed; add interesting inputs there (one expression /
JSON document per file).

Crash artifacts land in `fuzz/artifacts/<target>/`. Reproduce one with:

```sh
cargo fuzz run -s none <target> fuzz/artifacts/<target>/<artifact>
```

For long runs, fork mode keeps going past known crashers (see below) and
uses all cores:

```sh
cargo fuzz run -s none cel_compile -- \
  -fork=8 -ignore_crashes=1 -ignore_ooms=1 -ignore_timeouts=1
```

## Notes

- `overflow-checks` are intentionally off in the fuzz profile, matching
  production release builds (see the comment in `Cargo.toml`).
  `debug-assertions` stay on so assertion-style bugs still fire.

## Bugs found

1. **`cel` parser panic: depth-counter underflow** (upstream
   [cel-rust#305](https://github.com/cel-rust/cel-rust/issues/305)). A leading syntax error followed by deep nesting makes
   `RecursionListener::exit_expr` underflow (`self.depth -= 1` on a `u16`),
   panicking in builds with overflow checks. Fixed defensively in cala:
   `compile_program` converts compile-thread panics into parse errors
   (regression test: `parser_panic_surfaces_as_error`).
2. **`cel` parser exponential memory blowup** (upstream
   [cel-rust#306](https://github.com/cel-rust/cel-rust/issues/306), not fixed). A
   ~125-byte input — a 32-byte `!!(!!...` motif repeated 4 times — consumes
   >1 GiB during `Program::compile` (53 MiB at 3 repeats, 6 GiB at 5), OOMing
   the process. This is a DoS against any deployment that compiles
   user-supplied CEL (velocity controls, tx templates). No in-process
   mitigation is possible; needs an upstream parser fix.
3. **Panic in `timestamp.format(...)` builtin** (fixed). Invalid chrono
   format specifiers (e.g. `now.format('%Q')`) made `DelayedFormat`'s
   `Display` impl return `fmt::Error` and `.to_string()` panic. Now an
   `ExecutionError` (regression test:
   `invalid_format_on_timestamp_is_error_not_panic`).
4. **Panic coercing bytes to JSON** (fixed). `TryFrom<CelResult> for
   serde_json::Value` ended in `unimplemented!()` for `CelValue::Bytes`
   (e.g. `{'a': b'x'}`). Now a `ResultCoercionError` (regression test:
   `bytes_to_json_is_error_not_panic`).
5. **`BalanceSnapshot` arithmetic overflow panic** (found, not fixed here —
   scope of [#804](https://github.com/GaloyMoney/cala/pull/804)).
   `BalanceSnapshot::available` → `BalanceAmount::rollup` does unchecked `+`
   on `Decimal`; `rust_decimal`'s `Add` panics with "Addition overflowed"
   when layer balances sum past `Decimal::MAX` (~7.9e28). Reproduces from the
   fuzzer in seconds. The fix is `checked_add` in `rollup`, which is what #804
   covers.
