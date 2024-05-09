#![allow(clippy::blocks_in_conditions)]
mod config;

use async_trait::async_trait;
use cala_ledger_outbox_client::{
    CalaLedgerOutboxClient as Client, CalaLedgerOutboxClientConfig as ClientConfig,
};
use futures::StreamExt;
use tracing::instrument;

use super::runner::ImportJobRunnerDeps;
use crate::jobs::JobRunner;

pub use config::*;

pub const CALA_OUTBOX_IMPORT_JOB_TYPE: &str = "cala-outbox-import-job";

pub struct CalaOutboxImportJob {
    config: CalaOutboxImportConfig,
    _deps: ImportJobRunnerDeps,
}

impl CalaOutboxImportJob {
    pub fn new(config: CalaOutboxImportConfig, deps: &ImportJobRunnerDeps) -> Self {
        Self {
            config,
            _deps: deps.clone(),
        }
    }
}

#[async_trait]
impl JobRunner for CalaOutboxImportJob {
    #[instrument(name = "import_job.cala_outbox.run", skip(self), err)]
    async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "Running CalaOutboxImportJob with endpoint: {}",
            self.config.endpoint
        );
        let mut client = Client::connect(ClientConfig::from(&self.config)).await?;
        let mut stream = client.subscribe(Some(0)).await?;
        println!("created stream");
        while let Some(event) = stream.next().await {
            let message = event?;
            println!("message: {:?}", message);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
        Ok(())
    }
}
