use cala_extension::*;

#[derive(Default)]
pub struct AdditionalMutations;
impl MutationExtension for AdditionalMutations {}

#[async_graphql::Object]
impl AdditionalMutations {
    async fn test(&self) -> bool {
        true
    }
}
