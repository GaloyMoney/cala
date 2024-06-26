#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

pub use serde;
pub use sqlx;
pub use uuid;

mod id;

pub trait EsEntity {
    fn event_table_name() -> &'static str
    where
        Self: Sized;
}

pub use cala_es_entity_derive::EsEntity;
