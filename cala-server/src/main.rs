use cala_extension::*;
use cala_server::*;
use test_extension::TestExtension;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let extensions: Vec<Box<dyn CalaExtension>> = vec![Box::new(TestExtension {})];
    cli::run::<extensions::AdditionalMutations>(extensions).await
}
