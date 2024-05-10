#![allow(clippy::blocks_in_conditions)]
mod config;

use async_trait::async_trait;
use cala_ledger_outbox_client::{
    CalaLedgerOutboxClient as Client, CalaLedgerOutboxClientConfig as ClientConfig,
};
use futures::StreamExt;
use tracing::instrument;

use cala_ledger::{primitives::DataSourceId, CalaLedger};

use crate::jobs::{CurrentJob, JobRunner};

pub use config::*;

pub const CALA_OUTBOX_IMPORT_JOB_TYPE: &str = "cala-outbox-import-job";

pub struct CalaOutboxImportJob {
    config: CalaOutboxImportConfig,
    ledger: CalaLedger,
}

impl CalaOutboxImportJob {
    pub fn new(config: CalaOutboxImportConfig, ledger: &CalaLedger) -> Self {
        Self {
            config,
            ledger: ledger.clone(),
        }
    }
}

#[async_trait]
impl JobRunner for CalaOutboxImportJob {
    #[instrument(name = "import_job.cala_outbox.run", skip(self, current_job), err)]
    async fn run(&self, current_job: CurrentJob) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "Running CalaOutboxImportJob with endpoint: {}",
            self.config.endpoint
        );
        let mut client = Client::connect(ClientConfig::from(&self.config)).await?;
        let mut stream = client.subscribe(Some(0)).await?;
        while let Some(Ok(message)) = stream.next().await {
            println!("Received message: {:?}", message);
            let tx = current_job.pool().begin().await?;
            self.ledger
                .sync_outbox_event(tx, DataSourceId::from(current_job.id()), message)
                .await?;
        }
        Ok(())
    }
}
