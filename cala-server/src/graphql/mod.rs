mod account;
mod convert;
mod import_job;
mod journal;
mod primitives;
mod schema;
mod timestamp;

use async_graphql::*;

use cala_extension::MutationExtension;

pub use schema::*;

use crate::app::CalaApp;

#[derive(MergedObject, Default)]
pub struct Mutation<M: MutationExtension>(CoreMutations, M);

pub fn schema<M: MutationExtension>(
    app: Option<CalaApp>,
) -> Schema<Query, Mutation<M>, EmptySubscription> {
    let schema = Schema::build(Query, Mutation::<M>::default(), EmptySubscription);
    if let Some(app) = app {
        schema.data(app).finish()
    } else {
        schema.finish()
    }
}
