#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

mod id;

pub mod account;
pub mod account_set;
pub mod balance;
pub mod entry;
pub mod journal;
pub mod outbox;
pub mod param;
pub mod primitives;
pub mod transaction;
pub mod tx_template;
pub mod velocity;

#[cfg(feature = "graphql")]
pub mod graphql {
    pub use async_graphql;
    pub use base64;

    #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy)]
    #[serde(transparent)]
    pub struct UUID(uuid::Uuid);
    async_graphql::scalar!(UUID);
    impl<T: Into<uuid::Uuid>> From<T> for UUID {
        fn from(id: T) -> Self {
            let uuid = id.into();
            Self(uuid)
        }
    }
    impl From<&UUID> for uuid::Uuid {
        fn from(id: &UUID) -> Self {
            id.0
        }
    }
}
