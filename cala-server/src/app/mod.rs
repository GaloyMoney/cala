mod error;

use sqlx::{Pool, Postgres};

use cala_ledger::CalaLedger;

pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    _pool: Pool<Postgres>,
    ledger: CalaLedger,
}

impl CalaApp {
    pub fn new(pool: Pool<Postgres>, ledger: CalaLedger) -> Self {
        Self {
            _pool: pool,
            ledger,
        }
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }
}
