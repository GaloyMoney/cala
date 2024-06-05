use async_graphql::*;

mod cala_outbox_import;
pub mod core;

pub trait MutationExtensionMarker: Default + OutputType + ContainerType + 'static {}
pub trait QueryExtensionMarker: Default + OutputType + ContainerType + 'static {}
