mod config;
mod error;

use job::Jobs;
use sqlx::PgPool;

use cala_ledger::CalaLedger;

pub use config::*;
pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    _pool: PgPool,
    ledger: CalaLedger,
    jobs: Jobs,
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
                .build()
                .expect("JobSvcConfg"),
        )
        .await?;
        jobs.add_initializer(
            crate::extension::cala_outbox_import::CalaOutboxImportJobInitializer::new(
                ledger.clone(),
            ),
        );
        jobs.start_poll().await?;
        Ok(Self {
            _pool: pool,
            ledger,
            jobs,
        })
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }
}
