mod account;
mod balance;
mod convert;
mod job;
mod journal;
mod loader;
mod primitives;
mod schema;
mod timestamp;
mod transaction;
mod tx_template;

use async_graphql::{dataloader::*, *};

pub use job::Job;
pub use schema::*;

use crate::{app::CalaApp, extension::MutationExtensionMarker};
use loader::LedgerDataLoader;

pub fn schema<M: MutationExtensionMarker>(
    app: Option<CalaApp>,
) -> Schema<Query, CoreMutation<M>, EmptySubscription> {
    let schema = Schema::build(Query, CoreMutation::<M>::default(), EmptySubscription);
    if let Some(app) = app {
        schema
            .data(
                DataLoader::new(
                    LedgerDataLoader {
                        ledger: app.ledger().clone(),
                    },
                    tokio::task::spawn,
                )
                // Set delay to 0 as per https://github.com/async-graphql/async-graphql/issues/1306
                .delay(std::time::Duration::from_secs(0)),
            )
            .data(app)
            .finish()
    } else {
        schema.finish()
    }
}
