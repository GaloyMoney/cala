#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

pub mod app;
pub mod cli;
pub mod extensions;
pub mod graphql;
pub mod import_job;
mod jobs;
pub mod primitives;
pub mod server;
