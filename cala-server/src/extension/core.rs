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

pub fn job_registration(registry: &mut crate::job::JobRegistry) {
    registry.add_initializer::<super::cala_outbox_import::CalaOutboxImportJobInitializer>();
}
