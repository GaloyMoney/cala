mod config;
mod error;

use sqlx::PgPool;

use cala_ledger::CalaLedger;

use crate::{integration::*, job::*};
pub use config::*;
pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    pool: PgPool,
    ledger: CalaLedger,
    jobs: Jobs,
}

impl CalaApp {
    pub(crate) async fn run(
        pool: PgPool,
        config: AppConfig,
        ledger: CalaLedger,
        registry: JobRegistry,
    ) -> Result<Self, ApplicationError> {
        let mut jobs = Jobs::new(&pool, config.job_execution, registry);
        jobs.start_poll().await?;
        Ok(Self { pool, ledger, jobs })
    }

    pub fn integrations(&self) -> Integrations {
        Integrations::new(&self.pool)
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }
}
