pub mod config;
pub mod error;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
pub use tracing::instrument;
use tracing::Instrument;

pub use config::*;
use error::*;

use crate::{
    account::Accounts,
    account_set::AccountSets,
    balance::Balances,
    entry::Entries,
    journal::Journals,
    outbox::OutboxPublisher,
    primitives::TransactionId,
    transaction::{Transaction, Transactions},
    tx_template::{Params, TxTemplates},
    velocity::Velocities,
};

#[derive(Clone)]
pub struct CalaLedger {
    pool: PgPool,
    clock: ClockHandle,
    accounts: Accounts,
    account_sets: AccountSets,
    journals: Journals,
    transactions: Transactions,
    tx_templates: TxTemplates,
    entries: Entries,
    velocities: Velocities,
    balances: Balances,
    publisher: OutboxPublisher,
}

impl CalaLedger {
    #[instrument(name = "cala_ledger.init", skip_all)]
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
            sqlx::migrate!()
                .run(&pool)
                .instrument(tracing::info_span!("cala_ledger.migrations"))
                .await?;
        }

        let clock = config.clock;
        let publisher = OutboxPublisher::init(&pool, &clock).await?;
        let accounts = Accounts::new(&pool, &publisher, &clock);
        let journals = Journals::new(&pool, &publisher, &clock);
        let tx_templates = TxTemplates::new(&pool, &publisher, &clock);
        let transactions = Transactions::new(&pool, &publisher);
        let entries = Entries::new(&pool, &publisher);
        let balances = Balances::new(&pool, &publisher, &journals);
        let velocities = Velocities::new(&pool, &clock);
        let account_sets =
            AccountSets::new(&pool, &publisher, &accounts, &entries, &balances, &clock);
        Ok(Self {
            accounts,
            account_sets,
            journals,
            tx_templates,
            publisher,
            transactions,
            entries,
            balances,
            velocities,
            pool,
            clock,
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn clock(&self) -> &ClockHandle {
        &self.clock
    }

    pub async fn begin_operation(&self) -> Result<es_entity::DbOpWithTime<'static>, LedgerError> {
        let db_op = es_entity::DbOp::init_with_clock(&self.pool, &self.clock)
            .await?
            .with_clock_time();
        Ok(db_op)
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

    pub fn entries(&self) -> &Entries {
        &self.entries
    }

    pub fn transactions(&self) -> &Transactions {
        &self.transactions
    }

    #[instrument(
        name = "cala_ledger.post_transaction",
        skip(self, params),
        fields(tx_template_code)
    )]
    pub async fn post_transaction(
        &self,
        tx_id: TransactionId,
        tx_template_code: &str,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<Transaction, LedgerError> {
        let mut db = es_entity::DbOp::init_with_clock(&self.pool, &self.clock).await?;
        let transaction = self
            .post_transaction_in_op(&mut db, tx_id, tx_template_code, params)
            .await?;
        db.commit().await?;
        Ok(transaction)
    }

    #[instrument(
        name = "cala_ledger.post_transaction_in_op",
        skip(self, db)
        fields(transaction_id, external_id)
    )]
    pub async fn post_transaction_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        tx_id: TransactionId,
        tx_template_code: &str,
        params: impl Into<Params> + std::fmt::Debug,
    ) -> Result<Transaction, LedgerError> {
        let mut db = es_entity::OpWithTime::cached_or_db_time(db).await?;
        let time = db.now();
        let prepared_tx = self
            .tx_templates
            .prepare_transaction_in_op(&mut db, time, tx_id, tx_template_code, params.into())
            .await?;

        let transaction = self
            .transactions
            .create_in_op(&mut db, prepared_tx.transaction)
            .await?;

        let span = tracing::Span::current();
        span.record("transaction_id", transaction.id().to_string());
        span.record("external_id", &transaction.values().external_id);

        let entries = self
            .entries
            .create_all_in_op(&mut db, prepared_tx.entries)
            .await?;

        let account_ids = entries
            .iter()
            .map(|entry| entry.account_id)
            .collect::<Vec<_>>();
        let mappings = self
            .account_sets
            .fetch_mappings_in_op(&mut db, transaction.values().journal_id, &account_ids)
            .await?;

        self.velocities
            .update_balances_with_limit_enforcement_in_op(
                &mut db,
                transaction.created_at(),
                transaction.values(),
                &entries,
                &account_ids,
                &mappings,
            )
            .await?;

        self.balances
            .update_balances_in_op(
                &mut db,
                transaction.journal_id(),
                entries,
                transaction.effective(),
                transaction.created_at(),
                mappings,
            )
            .await?;
        Ok(transaction)
    }

    #[instrument(name = "cala_ledger.void_transaction", skip(self))]
    pub async fn void_transaction(
        &self,
        voiding_tx_id: TransactionId,
        existing_tx_id: TransactionId,
    ) -> Result<Transaction, LedgerError> {
        let mut db = self.begin_operation().await?;
        let transaction = self
            .void_transaction_in_op(&mut db, voiding_tx_id, existing_tx_id)
            .await?;
        db.commit().await?;
        Ok(transaction)
    }

    #[instrument(
        name = "cala_ledger.transaction_void",
        skip(self, db)
        fields(transaction_id, external_id)
    )]
    pub async fn void_transaction_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperationWithTime,
        voiding_tx_id: TransactionId,
        existing_tx_id: TransactionId,
    ) -> Result<Transaction, LedgerError> {
        let new_entries = self
            .entries
            .new_entries_for_voided_tx(voiding_tx_id, existing_tx_id)
            .await?;

        let transaction = self
            .transactions()
            .create_voided_tx_in_op(
                db,
                voiding_tx_id,
                existing_tx_id,
                new_entries.iter().map(|entry| entry.id),
            )
            .await?;

        let span = tracing::Span::current();
        span.record("transaction_id", transaction.id().to_string());
        span.record("external_id", &transaction.values().external_id);

        let entries = self.entries.create_all_in_op(db, new_entries).await?;

        let account_ids = entries
            .iter()
            .map(|entry| entry.account_id)
            .collect::<Vec<_>>();
        let mappings = self
            .account_sets
            .fetch_mappings_in_op(db, transaction.values().journal_id, &account_ids)
            .await?;

        self.velocities
            .update_balances_with_limit_enforcement_in_op(
                db,
                transaction.created_at(),
                transaction.values(),
                &entries,
                &account_ids,
                &mappings,
            )
            .await?;

        self.balances
            .update_balances_in_op(
                db,
                transaction.journal_id(),
                entries,
                transaction.effective(),
                transaction.created_at(),
                mappings,
            )
            .await?;
        Ok(transaction)
    }

    pub fn outbox(&self) -> &crate::outbox::ObixOutbox {
        self.publisher.inner()
    }

    pub fn register_outbox_listener(
        &self,
        start_after: Option<obix::EventSequence>,
    ) -> obix::out::PersistentOutboxListener<crate::outbox::OutboxEventPayload> {
        self.publisher.inner().listen_persisted(start_after)
    }

}
