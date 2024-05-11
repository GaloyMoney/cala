mod account;
mod convert;
mod import_job;
mod journal;
mod primitives;
mod schema;
mod timestamp;

use async_graphql::*;

pub use schema::*;

use crate::{app::CalaApp, extension::MutationExtensionMarker};

pub fn schema<M: MutationExtensionMarker>(
    app: Option<CalaApp>,
) -> Schema<Query, CoreMutation<M>, EmptySubscription> {
    let schema = Schema::build(Query, CoreMutation::<M>::default(), EmptySubscription);
    if let Some(app) = app {
        schema.data(app).finish()
    } else {
        schema.finish()
    }
}
