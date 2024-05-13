#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

pub mod app;
pub mod cli;
pub mod extension;
pub mod graphql;
pub mod job;
pub mod primitives;
pub mod server;

// Re exports
pub use async_graphql;
pub use async_trait;
pub use futures;
pub use tokio;
pub use tracing;

pub use cala_ledger::outbox;
pub use cala_ledger::CalaLedger;
