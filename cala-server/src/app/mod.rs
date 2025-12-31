mod config;
mod error;

use job::Jobs;
use sqlx::PgPool;

use cala_ledger::CalaLedger;

use super::extension::cala_outbox_import::{
    CalaOutboxImportJobConfig, CalaOutboxImportJobInitializer,
};

pub use config::*;
pub use error::*;

pub struct CalaApp {
    _pool: PgPool,
    ledger: CalaLedger,
    spawner: job::JobSpawner<CalaOutboxImportJobConfig>,
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
                .expect("JobSvcConfig"),
        )
        .await?;
        let spawner = jobs.add_initializer(CalaOutboxImportJobInitializer::new(ledger.clone()));
        jobs.start_poll().await?;
        Ok(Self {
            _pool: pool,
            ledger,
            spawner,
        })
    }

    pub fn ledger(&self) -> &CalaLedger {
        &self.ledger
    }

    pub fn spawner(&self) -> &job::JobSpawner<CalaOutboxImportJobConfig> {
        &self.spawner
    }
}
