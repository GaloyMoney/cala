use async_graphql::*;

pub mod core;

pub trait MutationExtensionMarker: Default + OutputType + ContainerType + 'static {}
pub trait QueryExtensionMarker: Default + OutputType + ContainerType + 'static {}
