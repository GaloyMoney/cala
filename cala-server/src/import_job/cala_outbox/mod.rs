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
            self.ledger
                .sync_outbox_event(DataSourceId::from(current_job.id), message)
                .await?;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
        Ok(())
    }
}
