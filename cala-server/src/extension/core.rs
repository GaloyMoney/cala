#[derive(async_graphql::SimpleObject, Default)]
pub struct CoreMutationExtension {
    #[graphql(flatten)]
    cala_outbox_import: super::cala_outbox_import::Mutation,
}

#[derive(async_graphql::SimpleObject, Default)]
pub struct MutationExtension {
    #[graphql(flatten)]
    core: CoreMutationExtension,
}
impl super::MutationExtensionMarker for MutationExtension {}

#[derive(Default)]
pub struct CoreQueryExtension;
#[async_graphql::Object]
impl CoreQueryExtension {
    async fn server_version(&self) -> &str {
        clap::crate_version!()
    }
}

#[derive(async_graphql::SimpleObject, Default)]
pub struct QueryExtension {
    #[graphql(flatten)]
    core: CoreQueryExtension,
}
impl super::QueryExtensionMarker for QueryExtension {}
