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
