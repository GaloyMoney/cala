#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

pub mod account;
pub mod account_set;
pub mod balance;
pub mod entity;
pub mod entry;
pub mod journal;
pub mod migrate;
pub mod transaction;
pub mod tx_template;

mod ledger;
pub mod outbox;

pub use ledger::*;

pub mod primitives {
    pub use cala_types::primitives::*;
}

pub mod query {
    #[derive(Debug)]
    pub struct PaginatedQueryArgs<T: std::fmt::Debug> {
        pub first: usize,
        pub after: Option<T>,
    }

    impl<T: std::fmt::Debug> Default for PaginatedQueryArgs<T> {
        fn default() -> Self {
            Self {
                first: 100,
                after: None,
            }
        }
    }

    pub struct PaginatedQueryRet<T, C> {
        pub entities: Vec<T>,
        pub has_next_page: bool,
        pub end_cursor: Option<C>,
    }
}

pub use primitives::*;
