use cala_server::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // let extensions: Vec<Box<dyn CalaExtension>> = vec![Box::new(TestExtension {})];
    cli::run::<extension::core::MutationExtension>().await
}
