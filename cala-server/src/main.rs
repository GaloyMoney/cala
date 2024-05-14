use cala_server::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run::<extension::core::MutationExtension>(extension::core::job_registration).await
}
