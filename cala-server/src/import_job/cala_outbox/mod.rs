mod config;

use async_trait::async_trait;

use super::runner::*;

pub use config::*;

pub struct CalaOutboxImportJob {
    config: CalaOutboxImportConfig,
}

impl CalaOutboxImportJob {
    pub fn new(config: CalaOutboxImportConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ImportJobRunner for CalaOutboxImportJob {
    async fn run(&self, _deps: ImportJobRunnerDeps) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "Running CalaOutboxImportJob with endpoint: {}",
            self.config.endpoint
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
        Ok(())
    }
}
