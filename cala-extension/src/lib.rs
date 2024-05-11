use async_graphql::*;

pub trait MutationExtension: Default + OutputType + ContainerType + 'static {}
pub trait CalaExtension {}
