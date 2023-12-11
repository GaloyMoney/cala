use sqlx::PgPool;

pub mod config;
pub mod error;

pub use config::*;
use error::*;

use crate::{
    account::Accounts,
    outbox::{server, Outbox},
};

pub struct CalaLedger {
    _pool: PgPool,
    accounts: Accounts,
    outbox_handle: Option<tokio::task::JoinHandle<()>>,
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

        let accounts = Accounts::new(&pool, outbox);
        Ok(Self {
            accounts,
            outbox_handle,
            _pool: pool,
        })
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    pub fn shutdown_outbox(&mut self) -> Result<(), LedgerError> {
        if let Some(handle) = self.outbox_handle.take() {
            handle.abort();
        }
        Ok(())
    }

    fn start_outbox_server(
        config: server::OutboxServerConfig,
        outbox: Outbox,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let _ = server::start(config, outbox).await;
        })
    }
}
