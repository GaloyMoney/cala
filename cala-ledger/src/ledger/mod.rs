use sqlx::PgPool;

pub mod config;
pub mod error;

pub use config::*;
use error::*;

use crate::account::Accounts;

pub struct CalaLedger {
    _pool: PgPool,
    accounts: Accounts,
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

        Ok(Self::new(pool))
    }

    fn new(pool: PgPool) -> Self {
        Self {
            accounts: Accounts::new(&pool),
            _pool: pool,
        }
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }
}
