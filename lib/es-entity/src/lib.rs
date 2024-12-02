#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

mod error;
mod events;
mod idempotent;
mod macros;
mod nested;
mod operation;
mod query;
mod traits;

pub mod prelude {
    pub use async_trait;
    pub use chrono;
    pub use serde_json;
    pub use sqlx;
    pub use uuid;
}

pub use error::*;
pub use es_entity_macros::expand_es_query;
pub use es_entity_macros::retry_on_concurrent_modification;
pub use es_entity_macros::EsEntity;
pub use es_entity_macros::EsEvent;
pub use es_entity_macros::EsRepo;
pub use events::*;
pub use idempotent::*;
pub use nested::*;
pub use operation::*;
pub use query::*;
pub use traits::*;

#[cfg(feature = "graphql")]
pub mod graphql {
    pub use async_graphql;
    pub use base64;

    #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy)]
    #[serde(transparent)]
    pub struct UUID(crate::prelude::uuid::Uuid);
    async_graphql::scalar!(UUID);
    impl<T: Into<crate::prelude::uuid::Uuid>> From<T> for UUID {
        fn from(id: T) -> Self {
            let uuid = id.into();
            Self(uuid)
        }
    }
    impl From<&UUID> for crate::prelude::uuid::Uuid {
        fn from(id: &UUID) -> Self {
            id.0
        }
    }
}
