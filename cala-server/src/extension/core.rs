#[derive(Default)]
pub struct MutationExtension;
impl super::MutationExtensionMarker for MutationExtension {}

#[async_graphql::Object]
impl MutationExtension {
    async fn test(&self) -> bool {
        true
    }
}
