#![allow(clippy::blocks_in_conditions)]

use async_trait::async_trait;
use cala_ledger::{primitives::DataSourceId, CalaLedger};
use cala_ledger_outbox_client::{
    CalaLedgerOutboxClient as Client, CalaLedgerOutboxClientConfig as ClientConfig,
};
use cala_types::outbox::EventSequence;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::job::*;

pub const CALA_OUTBOX_IMPORT_JOB_TYPE: JobType = JobType::new("cala-outbox-import-job");

#[derive(Default)]
pub struct CalaOutboxImportJobInitializer;

impl JobInitializer for CalaOutboxImportJobInitializer {
    fn job_type() -> JobType {
        CALA_OUTBOX_IMPORT_JOB_TYPE
    }

    fn init(
        &self,
        _job: Job,
        ledger: &CalaLedger,
    ) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>> {
        Ok(Box::new(CalaOutboxImportJob {
            ledger: ledger.clone(),
        }))
    }
}

pub struct CalaOutboxImportJob {
    ledger: CalaLedger,
}

#[derive(Serialize, Deserialize)]
pub struct CalaOutboxImportJobState {
    pub endpoint: String,
    pub last_synced: EventSequence,
}

#[async_trait]
impl JobRunner for CalaOutboxImportJob {
    #[instrument(name = "job.cala_outbox_import.run", skip(self, current_job), err)]
    async fn run(
        &self,
        mut current_job: CurrentJob,
    ) -> Result<JobCompletion, Box<dyn std::error::Error>> {
        let mut state = current_job
            .state::<CalaOutboxImportJobState>()?
            .expect("Job state");
        println!(
            "Executing CalaOutboxImportJob importing from endpoint: {}",
            state.endpoint
        );
        let mut client = Client::connect(ClientConfig::new(state.endpoint.clone())).await?;
        let mut stream = client.subscribe(Some(state.last_synced)).await?;
        loop {
            match stream.next().await {
                Some(Ok(message)) => {
                    let mut tx = current_job.pool().begin().await?;
                    state.last_synced = message.sequence;
                    current_job.update_state(&mut tx, &state).await?;
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
