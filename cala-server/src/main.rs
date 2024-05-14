use cala_server::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run::<extension::core::MutationExtension>(|r| {
        r.add_initializer::<extension::cala_outbox_import::CalaOutboxImportJobInitializer>()
    })
    .await
}
