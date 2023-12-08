use sqlx::{Acquire, PgPool, Postgres, Transaction};

pub mod error;

use error::*;

pub struct CalaLedger {
    pool: PgPool,
}

impl CalaLedger {
    pub async fn init(pool: PgPool) -> Result<Self, LedgerError> {
        sqlx::migrate!().run(&pool).await?;

        Ok(Self { pool })
    }
}
