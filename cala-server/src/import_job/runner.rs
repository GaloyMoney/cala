use async_trait::async_trait;

#[derive(Clone)]
pub struct ImportJobRunnerDeps {}

#[async_trait]
pub trait ImportJobRunner: Send {
    async fn run(&self) -> Result<(), Box<dyn std::error::Error>>;
}
