use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Hash, Clone, PartialEq, Eq, sqlx::Type)]
#[sqlx(transparent)]
pub struct JobType(pub &'static str);

#[async_trait]
pub trait JobRunner: Send {
    async fn run(&self) -> Result<(), Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait JobInitializer {
    fn job_type(&self) -> JobType;

    async fn init(&self, id: Uuid) -> Result<Box<dyn JobRunner>, Box<dyn std::error::Error>>;
}
