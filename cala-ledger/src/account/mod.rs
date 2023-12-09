//! [Account] holds a balance in a [Journal](crate::journal::Journal)
mod entity;
pub mod error;
mod repo;

pub use entity::*;
pub use repo::*;
