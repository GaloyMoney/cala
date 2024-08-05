pub mod config;
pub mod error;

use sqlx::PgPool;
use std::sync::{Arc, Mutex};
pub use tracing::instrument;

pub use config::*;
use error::*;

use crate::{
    account::Accounts,
    account_set::AccountSets,
    atomic_operation::*,
    balance::Balances,
    entry::Entries,
    journal::Journals,
    outbox::{server, EventSequence, Outbox, OutboxListener},
    primitives::TransactionId,
    transaction::{Transaction, Transactions},
    tx_template::{Params, TxTemplates},
    velocity::Velocities,
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
    account_sets: AccountSets,
    journals: Journals,
    transactions: Transactions,
    tx_templates: TxTemplates,
    entries: Entries,
    velocities: Velocities,
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
        let velocities = Velocities::new(&pool, outbox.clone());
        let account_sets = AccountSets::new(&pool, outbox.clone(), &accounts, &entries, &balances);
        Ok(Self {
            accounts,
            account_sets,
            journals,
            tx_templates,
            outbox,
            transactions,
            entries,
            balances,
            velocities,
            outbox_handle: Arc::new(Mutex::new(outbox_handle)),
            pool,
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn begin_operation<'a>(&self) -> Result<AtomicOperation<'a>, LedgerError> {
        Ok(AtomicOperation::init(&self.pool, &self.outbox).await?)
    }

    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    pub fn velocities(&self) -> &Velocities {
        &self.velocities
    }

    pub fn account_sets(&self) -> &AccountSets {
        &self.account_sets
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
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<Transaction, LedgerError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let transaction = self
            .post_transaction_in_op(&mut op, tx_id, tx_template_code, params)
            .await?;
        op.commit().await?;
        Ok(transaction)
    }

    #[instrument(
        name = "cala_ledger.transaction_post",
        skip(self, op)
        fields(transaction_id, external_id)
        err
    )]
    pub async fn post_transaction_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        tx_id: TransactionId,
        tx_template_code: &str,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<Transaction, LedgerError> {
        let prepared_tx = self
            .tx_templates
            .prepare_transaction(tx_id, tx_template_code, params.into())
            .await?;

        let transaction = self
            .transactions
            .create_in_op(op, prepared_tx.transaction)
            .await?;

        let span = tracing::Span::current();
        span.record("transaction_id", transaction.id().to_string());
        span.record("external_id", &transaction.values().external_id);

        let entries = self
            .entries
            .create_all_in_op(op, prepared_tx.entries)
            .await?;

        let account_ids = entries
            .iter()
            .map(|entry| entry.account_id)
            .collect::<Vec<_>>();
        let mappings = self
            .account_sets
            .fetch_mappings(transaction.values().journal_id, &account_ids)
            .await?;

        self.balances
            .update_balances_in_op(
                op,
                transaction.created_at(),
                transaction.journal_id(),
                entries,
                mappings,
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
    #[instrument(name = "cala_ledger.sync_outbox_event", skip(self, db))]
    pub async fn sync_outbox_event(
        &self,
        db: sqlx::Transaction<'_, sqlx::Postgres>,
        origin: DataSourceId,
        event: OutboxEvent,
    ) -> Result<(), LedgerError> {
        use crate::outbox::OutboxEventPayload::*;

        match event.payload {
            Empty => (),
            AccountCreated { account, .. } => {
                self.accounts
                    .sync_account_creation(db, event.recorded_at, origin, account)
                    .await?
            }
            AccountUpdated {
                account, fields, ..
            } => {
                self.accounts
                    .sync_account_update(db, event.recorded_at, origin, account, fields)
                    .await?
            }
            AccountSetCreated { account_set, .. } => {
                self.account_sets
                    .sync_account_set_creation(db, event.recorded_at, origin, account_set)
                    .await?
            }
            AccountSetUpdated {
                account_set,
                fields,
                ..
            } => {
                self.account_sets
                    .sync_account_set_update(db, event.recorded_at, origin, account_set, fields)
                    .await?
            }
            AccountSetMemberCreated {
                account_set_id,
                member_id,
                ..
            } => {
                self.account_sets
                    .sync_account_set_member_creation(
                        db,
                        event.recorded_at,
                        origin,
                        account_set_id,
                        member_id,
                    )
                    .await?
            }
            AccountSetMemberRemoved {
                account_set_id,
                member_id,
                ..
            } => {
                self.account_sets
                    .sync_account_set_member_removal(
                        db,
                        event.recorded_at,
                        origin,
                        account_set_id,
                        member_id,
                    )
                    .await?
            }
            JournalCreated { journal, .. } => {
                self.journals
                    .sync_journal_creation(db, event.recorded_at, origin, journal)
                    .await?
            }
            JournalUpdated {
                journal, fields, ..
            } => {
                self.journals
                    .sync_journal_update(db, event.recorded_at, origin, journal, fields)
                    .await?
            }
            TransactionCreated { transaction, .. } => {
                self.transactions
                    .sync_transaction_creation(db, event.recorded_at, origin, transaction)
                    .await?
            }
            TxTemplateCreated { tx_template, .. } => {
                self.tx_templates
                    .sync_tx_template_creation(db, event.recorded_at, origin, tx_template)
                    .await?
            }
            EntryCreated { entry, .. } => {
                self.entries
                    .sync_entry_creation(db, event.recorded_at, origin, entry)
                    .await?
            }
            BalanceCreated { balance, .. } => {
                self.balances
                    .sync_balance_creation(db, origin, balance)
                    .await?
            }
            BalanceUpdated { balance, .. } => {
                self.balances
                    .sync_balance_update(db, origin, balance)
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
