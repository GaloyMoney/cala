use async_trait::async_trait;

#[derive(Clone)]
pub struct ImportJobRunnerDeps {}

#[async_trait]
pub trait ImportJobRunner: Send {
    async fn run(&self, deps: ImportJobRunnerDeps) -> Result<(), Box<dyn std::error::Error>>;
}
