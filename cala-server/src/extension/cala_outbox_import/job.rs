#![allow(clippy::blocks_in_conditions)]

use async_trait::async_trait;
use cala_ledger::{primitives::DataSourceId, CalaLedger};
use cala_ledger_outbox_client::{
    CalaLedgerOutboxClient as Client, CalaLedgerOutboxClientConfig as ClientConfig,
};
use futures::StreamExt;
use obix::EventSequence;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use job::*;

pub const CALA_OUTBOX_IMPORT_JOB_TYPE: JobType = JobType::new("cala-outbox-import-job");

pub type CalaOutboxImportJobSpawner = JobSpawner<CalaOutboxImportJobConfig>;

#[derive(Serialize, Deserialize)]
pub struct CalaOutboxImportJobConfig {
    endpoint: String,
}
impl CalaOutboxImportJobConfig {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }
}

pub(crate) struct CalaOutboxImportJobInitializer {
    ledger: CalaLedger,
}
impl CalaOutboxImportJobInitializer {
    pub fn new(ledger: CalaLedger) -> Self {
        Self { ledger }
    }
}

impl JobInitializer for CalaOutboxImportJobInitializer {
    type Config = CalaOutboxImportJobConfig;
    fn job_type(&self) -> JobType {
        CALA_OUTBOX_IMPORT_JOB_TYPE
    }

    fn init(&self, job: &Job) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        Ok(Box::new(CalaOutboxImportJob {
            ledger: self.ledger.clone(),
            config: job.config()?,
        }))
    }
}

pub struct CalaOutboxImportJob {
    ledger: CalaLedger,
    config: CalaOutboxImportJobConfig,
}

#[derive(Default, Serialize, Deserialize)]
pub struct CalaOutboxImportJobState {
    pub last_synced: EventSequence,
}

#[async_trait]
impl JobRunner for CalaOutboxImportJob {
    #[instrument(name = "job.cala_outbox_import.run", skip(self, current_job))]
    async fn run(
        &self,
        mut current_job: CurrentJob,
    ) -> Result<JobCompletion, Box<dyn std::error::Error>> {
        let mut state = current_job
            .execution_state::<CalaOutboxImportJobState>()?
            .unwrap_or_default();
        println!(
            "Executing CalaOutboxImportJob importing from endpoint: {}",
            self.config.endpoint
        );
        let mut client = Client::connect(ClientConfig::new(self.config.endpoint.clone())).await?;
        let mut stream = client.subscribe(Some(state.last_synced)).await?;
        loop {
            match stream.next().await {
                Some(Ok(message)) => {
                    let mut tx = current_job.pool().begin().await?;
                    state.last_synced = message.sequence;
                    current_job
                        .update_execution_state_in_op(&mut tx, &state)
                        .await?;
                    self.ledger
                        .sync_outbox_event(
                            tx,
                            DataSourceId::from(uuid::Uuid::from(current_job.id())),
                            message,
                        )
                        .await?;
                }
                Some(Err(err)) => {
                    return Err(Box::new(err));
                }
                None => {
                    break;
                }
            }
        }
        Ok(JobCompletion::Complete)
    }
}
