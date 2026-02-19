#[derive(Default)]
pub struct MutationExtension;
#[async_graphql::Object]
impl MutationExtension {
    async fn mutation_version(&self) -> &str {
        clap::crate_version!()
    }
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
