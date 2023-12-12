mod account;
mod convert;
mod primitives;
mod schema;
mod timestamp;

use async_graphql::*;

pub use schema::*;

use crate::app::CalaApp;

pub fn schema(app: Option<CalaApp>) -> Schema<Query, Mutation, EmptySubscription> {
    let schema = Schema::build(Query, Mutation, EmptySubscription);
    if let Some(app) = app {
        schema.data(app).finish()
    } else {
        schema.finish()
    }
}
