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
