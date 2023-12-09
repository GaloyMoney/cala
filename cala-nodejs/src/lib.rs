#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

use napi::bindgen_prelude::*;

#[napi(object)]
pub struct CalaLedgerConfig {
  pub pg_con: String,
  pub max_connections: Option<u32>,
}

#[napi]
pub struct CalaLedger {
  inner: cala_ledger::CalaLedger,
}

#[napi]
impl CalaLedger {
  #[napi(factory)]
  pub async fn connect(config: CalaLedgerConfig) -> napi::Result<Self> {
    use cala_ledger::CalaLedgerConfig as Config;
    let mut builder = Config::builder();
    builder.pg_con(config.pg_con).exec_migrations(true);
    if let Some(n) = config.max_connections {
      builder.max_connections(n);
    }
    let config = builder.build().map_err(generic_napi_error)?;
    let inner = cala_ledger::CalaLedger::init(config)
      .await
      .map_err(generic_napi_error)?;
    Ok(Self { inner })
  }
}

fn generic_napi_error(e: impl std::fmt::Display) -> napi::Error {
  Error::from_reason(e.to_string())
}
