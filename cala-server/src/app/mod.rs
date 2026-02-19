mod config;
mod error;

use job::Jobs;
use sqlx::PgPool;

use cala_ledger::CalaLedger;

pub use config::*;
pub use error::*;

pub struct CalaApp {
    _pool: PgPool,
    ledger: CalaLedger,
    _jobs: job::Jobs,
}

impl CalaApp {
    pub(crate) async fn run(
        pool: PgPool,
        config: AppConfig,
        ledger: CalaLedger,
    ) -> Result<Self, ApplicationError> {
        let mut jobs = Jobs::init(
            job::JobSvcConfig::builder()
                .pool(pool.clone())
                .poller_config(config.jobs)
                .clock(ledger.clock().clone())
                .build()
                .expect("JobSvcConfig"),
        )
        .await?;
        jobs.start_poll().await?;

        Ok(Self {
            _pool: pool,
            ledger,
            _jobs: jobs,
        })
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }
}
