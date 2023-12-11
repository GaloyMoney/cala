mod config;

pub use config::*;

use super::account::*;

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
    if let Some(outbox) = config.outbox {
      if outbox.enabled {
        let mut outbox_config = cala_ledger::config::OutboxServerConfig::default();
        if let Some(n) = outbox.listen_port {
          outbox_config.listen_port = n;
        }
        builder.outbox(outbox_config);
      }
    }
    let config = builder.build().map_err(crate::generic_napi_error)?;
    let inner = cala_ledger::CalaLedger::init(config)
      .await
      .map_err(crate::generic_napi_error)?;
    Ok(Self { inner })
  }

  #[napi]
  pub fn accounts(&self) -> napi::Result<CalaAccounts> {
    Ok(CalaAccounts::new(self.inner.accounts()))
  }

  #[napi]
  pub async fn await_outbox_server(&self) -> napi::Result<()> {
    self
      .inner
      .await_outbox_handle()
      .await
      .map_err(crate::generic_napi_error)
  }
}
