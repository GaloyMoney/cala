pub mod account;
pub mod account_set;
pub mod balance;
mod convert;
mod job;
pub mod journal;
pub mod loader;
pub mod primitives;
mod schema;
mod timestamp;
pub mod transaction;
pub mod tx_template;

use async_graphql::{dataloader::*, *};

pub use job::Job;
pub use schema::*;

use crate::{app::CalaApp, extension::*};
use loader::LedgerDataLoader;

pub fn schema<Q: QueryExtensionMarker, M: MutationExtensionMarker>(
    app: Option<CalaApp>,
) -> Schema<CoreQuery<Q>, CoreMutation<M>, EmptySubscription> {
    let schema = Schema::build(
        CoreQuery::<Q>::default(),
        CoreMutation::<M>::default(),
        EmptySubscription,
    );
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
