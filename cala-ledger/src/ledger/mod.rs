pub mod config;
pub mod error;

use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tokio::{select, sync::oneshot};

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
    outbox_handle: Arc<Mutex<Option<tokio::task::JoinHandle<Result<(), LedgerError>>>>>,
    abort_sender: oneshot::Sender<()>,
    abort_receiver: oneshot::Receiver<()>,
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

        let (abort_sender, abort_receiver) = oneshot::channel::<()>();

        let accounts = Accounts::new(&pool, outbox.clone());
        let journals = Journals::new(&pool, outbox);
        Ok(Self {
            accounts,
            journals,
            outbox_handle: Arc::new(Mutex::new(outbox_handle)),
            abort_sender,
            abort_receiver,
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

        let handle = match handle {
            Some(handle) => handle,
            None => return Ok(()),
        };

        select! {
            result = handle => {
                result.expect("Couldn't await outbox handle")
            },

            _ = self.abort_receiver => {
                handle.abort();
                Ok(())
            },
        }
    }

    pub fn shutdown_outbox(&self) -> Result<(), LedgerError> {
        self.abort_sender.send(());
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
