pub mod config;
pub mod error;

use sqlx::PgPool;
use std::sync::{Arc, Mutex};

pub use config::*;
use error::*;

use crate::{
    account::Accounts,
    journal::Journals,
    outbox::{server, Outbox},
};

#[derive(Clone)]
pub struct CalaLedger {
    _pool: PgPool,
    accounts: Accounts,
    journals: Journals,
    #[allow(clippy::type_complexity)]
    outbox_handle: Arc<Mutex<Option<tokio::task::JoinHandle<Result<(), LedgerError>>>>>,
}

impl CalaLedger {
    pub async fn init(config: CalaLedgerConfig) -> Result<Self, LedgerError> {
        let pool = match (config.pool, config.pg_con) {
            (Some(pool), None) => pool,
            (None, Some(pg_con)) => {
                let mut pool_opts = sqlx::postgres::PgPoolOptions::new();
                if let Some(max_connections) = config.max_connections {
                    pool_opts = pool_opts.max_connections(max_connections);
                }
                pool_opts.connect(&pg_con).await?
            }
            _ => {
                return Err(LedgerError::ConfigError(
                    "One of pg_con or pool must be set".to_string(),
                ))
            }
        };
        if config.exec_migrations {
            sqlx::migrate!().run(&pool).await?;
        }

        let outbox = Outbox::new(&pool);
        let mut outbox_handle = None;
        if let Some(outbox_config) = config.outbox {
            outbox_handle = Some(Self::start_outbox_server(outbox_config, outbox.clone()));
        }

        let accounts = Accounts::new(&pool, outbox.clone());
        let journals = Journals::new(&pool, outbox);
        Ok(Self {
            accounts,
            journals,
            outbox_handle: Arc::new(Mutex::new(outbox_handle)),
            _pool: pool,
        })
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    pub fn journals(&self) -> &Journals {
        &self.journals
    }

    pub async fn await_outbox_handle(&self) -> Result<(), LedgerError> {
        let handle = { self.outbox_handle.lock().expect("poisened mutex").take() };
        if let Some(handle) = handle {
            return handle.await.expect("Couldn't await outbox handle");
        }
        Ok(())
    }

    pub fn shutdown_outbox(&mut self) -> Result<(), LedgerError> {
        if let Some(handle) = self.outbox_handle.lock().expect("poisened mutex").take() {
            handle.abort();
        }
        Ok(())
    }

    fn start_outbox_server(
        config: server::OutboxServerConfig,
        outbox: Outbox,
    ) -> tokio::task::JoinHandle<Result<(), LedgerError>> {
        tokio::spawn(async move {
            server::start(config, outbox).await?;
            Ok(())
        })
    }
}
