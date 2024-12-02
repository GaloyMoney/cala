//! # cala-ledger
//!
//! This crate provides a set of primitives for implementing an SQL-compatible
//! double-entry accounting system. This system is engineered specifically is
//! for dealing with money and building financial products.
//!
//! Visit the [website of the Cala project](https://cala.sh) for more info and
//! tutorials.
//!
//! ## Quick Start
//!
//! Here is how to initialize a ledger create a primitive template and post a transaction.
//! This is a toy example that brings all pieces together end-to-end.
//! Not recommended for real use.
//! ```rust
//! use cala_ledger::{account::*, journal::*, tx_template::*, *};
//! use rust_decimal::Decimal;
//! use uuid::uuid;
//!
//! async fn init_cala(journal_id: JournalId) -> anyhow::Result<CalaLedger, anyhow::Error> {
//!     let cala_config = CalaLedgerConfig::builder()
//!         .pg_con("postgres://user:password@localhost:5432/pg")
//!         // .exec_migrations(true) # commented out for execution in CI
//!         .build()?;
//!     let cala = CalaLedger::init(cala_config).await?;
//!
//!     // Initialize the journal - all entities are constructed via builders
//!     let new_journal = NewJournal::builder()
//!         .id(journal_id)
//!         .name("Ledger")
//!         .build()
//!         .expect("Couldn't build NewJournal");
//!     let _ = cala.journals().create(new_journal).await;
//!
//!     // Initialize an income omnibus account
//!     let main_account_id = uuid!("00000000-0000-0000-0000-000000000001");
//!     let new_account = NewAccount::builder()
//!         .id(main_account_id)
//!         .name("Income")
//!         .code("Income")
//!         .build()?;
//!     cala.accounts().create(new_account).await?;
//!
//!     // Create the trivial 'income' template
//!     let params = vec![
//!         NewParamDefinition::builder()
//!             .name("sender_account_id")
//!             .r#type(ParamDataType::Uuid)
//!             .build()?,
//!         NewParamDefinition::builder()
//!             .name("units")
//!             .r#type(ParamDataType::Decimal)
//!             .build()?,
//!     ];
//!
//!     let entries = vec![
//!         NewTxTemplateEntry::builder()
//!             .entry_type("'INCOME_DR'")
//!             .account_id("params.sender_account_id")
//!             .layer("SETTLED")
//!             .direction("DEBIT")
//!             .units("params.units")
//!             .currency("'BTC'")
//!             .build()?,
//!         NewTxTemplateEntry::builder()
//!             .entry_type("'INCOME_CR'")
//!             .account_id(format!("uuid('{}')", main_account_id))
//!             .layer("SETTLED")
//!             .direction("CREDIT")
//!             .units("params.units")
//!             .currency("'BTC'")
//!             .build()?,
//!     ];
//!
//!     let tx_code = "GENERAL_INCOME";
//!     let new_template = NewTxTemplate::builder()
//!         .id(uuid::Uuid::new_v4())
//!         .code(tx_code)
//!         .params(params)
//!         .transaction(
//!             NewTxTemplateTransaction::builder()
//!                 .effective("date()")
//!                 .journal_id(format!("uuid('{}')", journal_id))
//!                 .build()?,
//!         )
//!         .entries(entries)
//!         .build()?;
//!
//!     cala.tx_templates().create(new_template).await?;
//!     Ok(cala)
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let journal_id = JournalId::from(uuid!("00000000-0000-0000-0000-000000000001"));
//!     let cala = init_cala(journal_id).await?;
//!     // The account that is sending to the general income account
//!     let sender_account_id = AccountId::new();
//!     let sender_account = NewAccount::builder()
//!         .id(sender_account_id)
//!         .name(format!("Sender-{}", sender_account_id))
//!         .code(format!("Sender-{}", sender_account_id))
//!         .build()?;
//!     cala.accounts().create(sender_account).await?;
//!     // Prepare the input parameters that the template requires
//!     let mut params = Params::new();
//!     params.insert("sender_account_id", sender_account_id);
//!     params.insert("units", Decimal::ONE);
//!     // Create the transaction via the template
//!     cala.post_transaction(TransactionId::new(), "GENERAL_INCOME", params)
//!         .await?;
//!
//!     let account_balance = cala
//!         .balances()
//!         .find(journal_id, sender_account_id, "BTC".parse()?)
//!         .await?;
//!
//!     let expected_balance = Decimal::new(-1, 0); // Define the expected balance
//!     assert_eq!(account_balance.settled(), expected_balance);
//!     Ok(())
//! }
//! ```

#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

mod cel_context;
mod param;

pub mod account;
pub mod account_set;
pub mod balance;
pub mod entry;
pub mod journal;
pub mod ledger_operation;
pub mod migrate;
pub mod transaction;
pub mod tx_template;
pub mod velocity;

mod ledger;
pub mod outbox;

pub use ledger::*;
pub use ledger_operation::*;

pub mod primitives {
    pub use cala_types::primitives::*;
}

pub use es_entity;

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
