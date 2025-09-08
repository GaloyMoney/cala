use async_graphql::*;

pub(crate) mod cala_outbox_import;
pub mod core;

pub trait MutationExtensionMarker: Default + OutputType + ContainerType + 'static {}
pub trait QueryExtensionMarker: Default + OutputType + ContainerType + 'static {}
