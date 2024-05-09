use async_trait::async_trait;
use uuid::Uuid;

pub type JobType = &'static str;

pub struct JobTemplate {
    pub job_type: JobType,
    pub id: Uuid,
}

impl JobTemplate {
    pub fn new(job_type: JobType, id: impl Into<Uuid>) -> Self {
        Self {
            job_type,
            id: id.into(),
        }
    }
}

#[async_trait]
pub trait JobRunner: Send + Sync + 'static {
    async fn run(&self) -> Result<(), Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait JobInitializer: Send + Sync + 'static {
    async fn init(&self, id: Uuid) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}
