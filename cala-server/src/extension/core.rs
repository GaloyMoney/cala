#[derive(async_graphql::SimpleObject, Default)]
pub struct CoreMutationExtension {
    hello: String,
}

#[derive(async_graphql::SimpleObject, Default)]
pub struct MutationExtension {
    #[graphql(flatten)]
    core: CoreMutationExtension,
}
impl super::MutationExtensionMarker for MutationExtension {}
