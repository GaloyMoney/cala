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
    /// Initialize the ledger.
    ///
    /// The streaming EC account-set balance rollup is registered against the
    /// caller-owned `jobs` here; the caller drives its lifecycle (call
    /// `start_poll` to run it, and shut it down). The rollup only runs once
    /// `jobs` is polled.
    #[instrument(name = "cala_ledger.init", skip_all)]
    pub async fn init(config: CalaLedgerConfig, jobs: &mut job::Jobs) -> Result<Self, LedgerError> {
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
        let account_sets = AccountSets::new(&pool, &publisher, &accounts, &balances, &clock);

        crate::ec_rollup::register_ec_balance_rollup(jobs, publisher.inner(), &balances, &entries)
            .await?;

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

        let journal_id = transaction.values().journal_id;
        self.balances
            .lock_entry_balances_in_op(&mut db, journal_id, &prepared_tx.entries)
            .await?;

        // The walk reads only the membership graph, so it can run before
        // the entry insert; it also takes the per-balance locks for the
        // non-EC ancestors it resolves.
        let mappings = self
            .account_sets
            .fetch_mappings_in_op(&mut db, journal_id, &prepared_tx.entries)
            .await?;

        let entries = self
            .entries
            .create_all_in_op(&mut db, prepared_tx.entries)
            .await?;

        let account_ids = entries
            .iter()
            .map(|entry| entry.account_id)
            .collect::<Vec<_>>();

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

    /// Snapshot the streaming EC-balance rollup's position: its committed
    /// checkpoint against the persistent outbox frontier. Read-only and
    /// cheap — suitable for polling as a stream-lag SLO metric.
    ///
    /// Works from any node: both sides are read from the database, not
    /// from in-process state (the rollup runs on one node cluster-wide).
    #[instrument(name = "cala_ledger.ec_rollup_status", skip_all)]
    pub async fn ec_rollup_status(&self) -> Result<crate::EcRollupStatus, LedgerError> {
        crate::ec_rollup::rollup_status(&self.pool).await
    }

    /// Await the streaming EC-balance rollup catching up to the outbox
    /// frontier snapshotted at call time.
    ///
    /// On `Ok(())`: every transaction committed before this call — and
    /// every posting that had already been assigned an outbox sequence,
    /// committed or still in flight — is folded into EC balances (settled
    /// and effective, same commit) and visible to subsequent reads.
    ///
    /// ## Why an argument-less fence suffices after closing a period
    ///
    /// - Outbox sequences are assigned early in the posting flow (entry
    ///   events are published at entry insert, *before* velocity
    ///   enforcement), so any posting that saw a period as *open* holds a
    ///   sequence at or below the frontier this fence snapshots.
    /// - Delivery is gapless: the rollup cannot advance past sequence `N`
    ///   until `N` is resolved (applied or aborted), so awaiting
    ///   `checkpoint ≥ frontier` transitively waits for every such
    ///   in-flight posting.
    ///
    /// Consequently `close_books(); await_ec_caught_up(..);` has no
    /// straggler hole: reports generated from EC balances after the fence
    /// reflect every posting that passed the pre-close check.
    ///
    /// The wait polls the rollup job's persisted checkpoint in the
    /// database with backoff. The checkpoint only ever *trails* the
    /// applied state (by up to the obix checkpoint interval over
    /// skip-only stretches), so the fence may wait marginally longer than
    /// necessary but never returns early.
    ///
    /// Each call anchors to its own call-time frontier. `Ok(())` does
    /// *not* imply a subsequent [`ec_rollup_status`](Self::ec_rollup_status)
    /// reports caught-up: applying the awaited batches publishes
    /// `BalanceUpdated` events of its own, which the rollup crosses as
    /// skips shortly after (see [`crate::EcRollupStatus::lag`]).
    ///
    /// `timeout` is mandatory: a stopped or wedged rollup surfaces as
    /// [`LedgerError::EcCaughtUpTimeout`] carrying the observed
    /// checkpoint and frontier — alertable, never a silent hang.
    #[instrument(name = "cala_ledger.await_ec_caught_up", skip_all, fields(timeout = ?timeout))]
    pub async fn await_ec_caught_up(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), LedgerError> {
        crate::ec_rollup::await_caught_up(&self.pool, timeout).await
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
