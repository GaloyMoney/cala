pub mod config;
pub mod error;

use sqlx::{Acquire, PgPool, Postgres, Transaction as DbTransaction};
use std::sync::{Arc, Mutex};
pub use tracing::instrument;

pub use config::*;
use error::*;

use crate::{
    account::Accounts,
    balance::Balances,
    entry::Entries,
    journal::Journals,
    outbox::{server, EventSequence, Outbox, OutboxListener},
    primitives::TransactionId,
    transaction::{Transaction, Transactions},
    tx_template::{TxParams, TxTemplates},
};
#[cfg(feature = "import")]
mod import_deps {
    pub use crate::primitives::DataSourceId;
    pub use cala_types::outbox::OutboxEvent;
}
#[cfg(feature = "import")]
use import_deps::*;

#[derive(Clone)]
pub struct CalaLedger {
    pool: PgPool,
    accounts: Accounts,
    journals: Journals,
    transactions: Transactions,
    tx_templates: TxTemplates,
    entries: Entries,
    balances: Balances,
    outbox: Outbox,
    #[allow(clippy::type_complexity)]
    outbox_handle: Arc<Mutex<Option<tokio::task::JoinHandle<Result<(), LedgerError>>>>>,
}

impl CalaLedger {
    pub async fn init(config: CalaLedgerConfig) -> Result<Self, LedgerError> {
        let pool = match (config.pool, config.pg_con) {
            (Some(pool), None) => pool,
            (None, Some(pg_con)) => {
                let mut pool_opts = sqlx::postgres::PgPoolOptions::new();
                if let Some(max_connections) = config.max_connections {
                    pool_opts = pool_opts.max_connections(max_connections);
                }
                pool_opts.connect(&pg_con).await?
            }
            _ => {
                return Err(LedgerError::ConfigError(
                    "One of pg_con or pool must be set".to_string(),
                ))
            }
        };
        if config.exec_migrations {
            sqlx::migrate!().run(&pool).await?;
        }

        let outbox = Outbox::init(&pool).await?;
        let mut outbox_handle = None;
        if let Some(outbox_config) = config.outbox {
            outbox_handle = Some(Self::start_outbox_server(outbox_config, outbox.clone()));
        }

        let accounts = Accounts::new(&pool, outbox.clone());
        let journals = Journals::new(&pool, outbox.clone());
        let tx_templates = TxTemplates::new(&pool, outbox.clone());
        let transactions = Transactions::new(&pool, outbox.clone());
        let entries = Entries::new(&pool, outbox.clone());
        let balances = Balances::new(&pool, outbox.clone());
        Ok(Self {
            accounts,
            journals,
            tx_templates,
            outbox,
            transactions,
            entries,
            balances,
            outbox_handle: Arc::new(Mutex::new(outbox_handle)),
            pool,
        })
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    pub fn journals(&self) -> &Journals {
        &self.journals
    }

    pub fn tx_templates(&self) -> &TxTemplates {
        &self.tx_templates
    }

    pub fn balances(&self) -> &Balances {
        &self.balances
    }

    pub fn transactions(&self) -> &Transactions {
        &self.transactions
    }

    pub async fn post_transaction(
        &self,
        tx_id: TransactionId,
        tx_template_code: &str,
        params: Option<impl Into<TxParams> + std::fmt::Debug>,
    ) -> Result<Transaction, LedgerError> {
        let tx = self.pool.begin().await?;
        self.post_transaction_in_tx(tx, tx_id, tx_template_code, params)
            .await
    }

    #[instrument(name = "cala_ledger.post_transaction", skip(self, tx))]
    pub async fn post_transaction_in_tx(
        &self,
        mut tx: DbTransaction<'_, Postgres>,
        tx_id: TransactionId,
        tx_template_code: &str,
        params: Option<impl Into<TxParams> + std::fmt::Debug>,
    ) -> Result<Transaction, LedgerError> {
        let prepared_tx = self
            .tx_templates
            .prepare_transaction(
                tx_id,
                tx_template_code,
                params.map(|p| p.into()).unwrap_or_default(),
            )
            .await?;
        let (transaction, tx_event) = self
            .transactions
            .create_in_tx(&mut tx, prepared_tx.transaction)
            .await?;
        let (entries, entry_events) = self
            .entries
            .create_all(&mut tx, prepared_tx.entries)
            .await?;
        let balance_events = self
            .balances
            .update_balances(
                tx.begin().await?,
                transaction.created_at(),
                transaction.journal_id(),
                entries,
            )
            .await?;
        self.outbox
            .persist_events(
                tx,
                std::iter::once(tx_event)
                    .chain(entry_events)
                    .chain(balance_events),
            )
            .await?;
        Ok(transaction)
    }

    pub async fn register_outbox_listener(
        &self,
        start_after: Option<EventSequence>,
    ) -> Result<OutboxListener, LedgerError> {
        Ok(self.outbox.register_listener(start_after).await?)
    }

    #[cfg(feature = "import")]
    #[instrument(name = "cala_ledger.sync_outbox_event", skip(self, tx))]
    pub async fn sync_outbox_event(
        &self,
        tx: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        event: OutboxEvent,
    ) -> Result<(), LedgerError> {
        use crate::outbox::OutboxEventPayload::*;

        match event.payload {
            Empty => (),
            AccountCreated { account, .. } => {
                self.accounts
                    .sync_account_creation(tx, event.recorded_at, origin, account)
                    .await?
            }
            JournalCreated { journal, .. } => {
                self.journals
                    .sync_journal_creation(tx, event.recorded_at, origin, journal)
                    .await?
            }
            TransactionCreated { transaction, .. } => {
                self.transactions
                    .sync_transaction_creation(tx, event.recorded_at, origin, transaction)
                    .await?
            }
            TxTemplateCreated { tx_template, .. } => {
                self.tx_templates
                    .sync_tx_template_creation(tx, event.recorded_at, origin, tx_template)
                    .await?
            }
            EntryCreated { entry, .. } => {
                self.entries
                    .sync_entry_creation(tx, event.recorded_at, origin, entry)
                    .await?
            }
            BalanceCreated { balance, .. } => {
                self.balances
                    .sync_balance_creation(tx, origin, balance)
                    .await?
            }
            BalanceUpdated { balance, .. } => {
                self.balances
                    .sync_balance_update(tx, origin, balance)
                    .await?
            }
        }
        Ok(())
    }

    pub async fn await_outbox_handle(&self) -> Result<(), LedgerError> {
        let handle = { self.outbox_handle.lock().expect("poisened mutex").take() };
        if let Some(handle) = handle {
            return handle.await.expect("Couldn't await outbox handle");
        }
        Ok(())
    }

    pub fn shutdown_outbox(&mut self) -> Result<(), LedgerError> {
        if let Some(handle) = self.outbox_handle.lock().expect("poisened mutex").take() {
            handle.abort();
        }
        Ok(())
    }

    fn start_outbox_server(
        config: server::OutboxServerConfig,
        outbox: Outbox,
    ) -> tokio::task::JoinHandle<Result<(), LedgerError>> {
        tokio::spawn(async move {
            server::start(config, outbox).await?;
            Ok(())
        })
    }
}
