mod account;
mod balance;
mod convert;
mod job;
mod journal;
mod primitives;
mod schema;
mod timestamp;
mod transaction;
mod tx_template;

use async_graphql::*;

pub use job::Job;
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
