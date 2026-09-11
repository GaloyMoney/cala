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

## Entry events

`OutboxEventPayload::EntryCreated { entry, effective }` carries the owning
transaction's accounting date alongside `EntryValues`. On the wire, `effective`
is an ISO date (`YYYY-MM-DD`) at the same level as `entry`. It is copied from the
transaction when posting, not derived from the outbox event's recording time:
a backdated posting can be recorded today and affect an earlier accounting day.
Consumers can group entries by accounting date without joining transaction events.

The transaction remains the authority for this date; callers cannot set it
independently per entry. `EntryValues`, persisted entry entity events, and the
entry database schema do not include it. Transaction and entry events commit
atomically, but streaming batches can split a posting's event group; carrying
`effective` does not guarantee posting-atomic delivery.

### Upgrading entry event consumers

`effective` is required when deserializing `EntryCreated`. Upgrading an existing
outbox requires a coordinated stop of writers and consumers: backfill older
`entry_created` payloads from the owning `cala_transactions.effective` (matched
by `payload.entry.transaction_id`), then restart with the new version. Old
writers cannot coexist with new consumers because they omit the required date.
Archived copies need the same enrichment before replay. There is no fallback
to the recording date or a sentinel date, and this release does not
automatically backfill existing outbox data.

Rust consumers that destructure `EntryCreated { entry }` must include `effective`
or use `EntryCreated { entry, .. }`. The entry-only
`From<&EntryEvent> for OutboxEventPayload` conversion is removed: constructing a
public entry event requires the owning transaction's date.

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
