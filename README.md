# Cala

Cala is a robust ledger library developed by Galoy, designed to handle complex financial transactions and accounting operations. It provides a flexible and scalable solution for managing financial records with strong consistency guarantees.

Cala is distributed as a Rust library that you embed in your own service — it does not run as a standalone server.

## Features

### Core Capabilities

- **Double-Entry Accounting**: Built-in support for double-entry bookkeeping principles ensuring accurate financial records
- **SQL-Compatible**: Engineered to work with SQL databases (PostgreSQL) for robust data persistence and querying
- **Strong Consistency**: Ensures accuracy and reliability of financial records
- **Real-time Processing**: Efficient transaction processing suitable for production financial systems

### API & Integration

- **Rust Library**: Embed the ledger directly in your Rust service via the `cala-ledger` crate
- **Transaction Templates**: Customizable transaction templates for common financial operations, parameterized with CEL expressions
- **Multi-Currency Support**: Handle transactions across different currencies
- **Event Sourcing**: Persistent outbox for reliably streaming ledger events to downstream consumers

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
cala-ledger = "0.20"
```

Then initialize the ledger with a PostgreSQL connection pool:

```rust
use cala_ledger::{CalaLedger, CalaLedgerConfig};

let pool = sqlx::postgres::PgPoolOptions::new()
    .max_connections(20)
    .connect("postgres://user:password@localhost:5432/pg")
    .await?;

let cala_config = CalaLedgerConfig::builder()
    .pool(pool)
    .exec_migrations(true)
    .build()?;
let cala = CalaLedger::init(cala_config).await?;
```

For a complete working example — including creating accounts, transaction templates, and posting transactions — see [examples/rust](./examples/rust) and run it with:

```bash
make reset-deps rust-example
```

## Developing

### Dependencies

#### Nix package manager

- Recommended install method using https://github.com/DeterminateSystems/nix-installer
  ```
  curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install
  ```

#### direnv >= 2.30.0

- Recommended install method from https://direnv.net/docs/installation.html:
  ```
  curl -sfL https://direnv.net/install.sh | bash
  echo "eval \"\$(direnv hook bash)\"" >> ~/.bashrc
  source ~/.bashrc
  ```

### Testing

Run unit tests with:

```bash
make reset-deps next-watch
```

## Errors

Cala uses the error lanes re-exported at `cala_ledger::errlanes`. Methods that
can reject caller input return `Fail<DomainRejection, lanes!(Transient, Fatal)>`.
Match `Fail::Rejected` for outcomes such as a duplicate account code, a missing
requested account, a velocity limit, or an invalid posting. Fault-only methods
(including list/bulk reads, ledger initialization, and EC status refresh) return
`CalaFault`. Bulk reads represent absent entities by omission from their result.

```rust,ignore
use cala_ledger::{account::error::AccountCodeNotFound, errlanes::ResultExt};

match cala.accounts().find_by_code(code).await.rejected()? {
    Ok(account) => use_account(account),
    Err(AccountCodeNotFound(code)) => request_another_code(code),
}
```

`ResultExt::rejected()` exposes the domain outcome while `?` propagates faults.
Use `.widen()` to lift between rejection families, `.map_rejected()` to add
caller context without altering fault lanes, and `.narrow_rejected()` only when
an internal invariant means no caller can correct a rejection. Fault payloads
are diagnostic sources, not a public branching contract. Required internal
repository reads treat missing rows as invariants; public lookups that reject
absence use optional repository reads and construct a typed rejection.

Single posting exposes `PostingRejection`; batch posting exposes
`BatchPostingRejection`. Both have three variants: `Prepare`, `Validate`, and
`Apply`, each carrying the rejection contract owned by that phase.

Preparation covers template lookup, parameter binding/defaults, expression
evaluation, balancing, and the distinct-balance budget. Its CEL and parameter
causes are boxed, retaining parameter names and original diagnostic sources.
Validation covers direct account and journal checks. Application covers ancestor
locks, velocity enforcement, and write conflicts. Velocity owns
`EnforceVelocityRejection::{Cel, LimitExceeded}`; posting exposes it through
`ApplyPostingRejection::Velocity`.

Velocity control attachment exposes `AttachVelocityControlRejection` with
`ControlNotFound`, `Param`, `Default`, and `Cel` variants. Parameter coercion,
parameter-default evaluation, and balance-limit evaluation retain their nested
causes and delegate diagnostic codes and levels to them. A CEL source converts
to `Cel` by default; parameter binding explicitly selects `Default`.

Preparation evaluation and direct validation retain `PostingRef { index, tx_id }`
and the `CALA_POSTING_REJECTED` code. Template absence, budget failures, ancestor
locks, velocity enforcement, and apply-time conflicts remain unattributed.
Faults carry no posting index. The phase wrappers delegate diagnostic codes and
display text to their causes.

Batch preparation composes the single-posting preparation cases and adds
duplicate IDs within the submitted batch. Batch validation and application use
the same contracts as single posting. The single-to-batch conversion preserves
phase, context, and diagnostics.

Service contracts describe the operation: template creation has ID/code
conflicts, lookup has only a missing-code leaf, and pure preparation has no
fault carrier. Account/journal persistence excludes primary-key conflicts.
Account-only membership addition excludes graph cycles/depth and journal
mismatch; removal excludes addition-only failures.

Choose boundaries from the public use case inward. Phase helpers return their
phase contract, and independent components return their own contract. Velocity
enforcement does not return a posting rejection. Use `?` for supported total
conversions and `.widen()` across rejection/lane boundaries; attach runtime
context with direct constructors in `map_err`.

Nest errors along meaningful phase and component boundaries, without a fixed
depth cap. Avoid enums that merely mirror incidental helper calls, and avoid
flattening independent component failures into their caller. Use
`#[errlanes::compose(Source)]` for unchanged union inclusion, such as extending single
preparation with batch-only checks. Do not enumerate CEL variants just to copy
them into a posting contract.

Use `derive(Lift)` for structural mappings between contracts. When a variant's
payload needs conversion, `#[lift(Source::Variant, into)]` converts it into the
destination payload type before wrapping it. Batch posting uses this for its
preparation phase and forwards validation/application unchanged. Keep
`#[rejection(delegate)]` on these wrappers so diagnostics come from their payloads.

CEL parsing returns `CelParseRejection`. Both `evaluate` and `try_evaluate<T>`
return the same `CelConversionRejection`, covering
evaluation and all supported result conversions. Individual targets produce
only a subset of those cases. `evaluate` performs no target conversion but shares
the same rejection type. Compiled expressions cannot report parse errors during
evaluation. Pure helpers
return bare rejection results; graph validation additionally has a fatal lane
for corrupt stored graphs. EC waits
expose `EcCaughtUpTimeout` with the observed positions and deadline; a missing
registered rollup is a fault.

SQL failures retain errlanes' classification. Stored JSON/currency decode
failures are `Fatal(CorruptState)`; configuration/migration failures are
`Fatal(Config)`. Errors entering the job runner retain their lanes through the
boxed handler boundary. Callers own retry policy and transaction boundaries.

Laned tracing spans record `error.lane`, `error.code`, `error.level`,
`exception.message`, and `exception.type`. Rejection telemetry uses its stable
code rather than rendering caller input. This is a breaking API change: replace
matches on the former `*Error` enums with `Fail::Rejected(*Rejection::...)`, and
use `CalaFault` for methods whose signatures no longer carry domain outcomes.
