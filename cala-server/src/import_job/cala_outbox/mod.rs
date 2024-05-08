#![allow(clippy::blocks_in_conditions)]
mod config;

use async_trait::async_trait;
use cala_ledger_outbox_client::{
    CalaLedgerOutboxClient as Client, CalaLedgerOutboxClientConfig as ClientConfig,
};
use serde_json::Value;
use tracing::instrument;

use super::runner::ImportJobRunnerDeps;
use crate::jobs::JobRunner;

pub use config::*;

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
        let _stream = client.subscribe(None).await?;
        println!("created stream");
        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
        Ok(())
    }
}
