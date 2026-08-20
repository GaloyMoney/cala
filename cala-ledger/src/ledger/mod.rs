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
    account_set_member::AccountSetMembers,
    balance::Balances,
    entry::Entries,
    journal::Journals,
    outbox::OutboxPublisher,
    posting::{PostingInput, Postings},
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
    postings: Postings,
    publisher: OutboxPublisher,
    ec_rollup: obix::out::RegisteredEventHandler<
        crate::outbox::OutboxEventPayload,
        crate::outbox::CalaMailboxTables,
    >,
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
        let account_set_members = AccountSetMembers::new(&pool, &publisher);
        let accounts = Accounts::new(&pool, &publisher, &account_set_members, &clock);
        let journals = Journals::new(&pool, &publisher, &clock);
        let tx_templates = TxTemplates::new(&pool, &publisher, &clock);
        let transactions = Transactions::new(&pool);
        let entries = Entries::new(&pool);
        let balances = Balances::new(&pool, &journals);
        let velocities = Velocities::new(&pool, &clock);
        let account_sets = AccountSets::new(
            &pool,
            &publisher,
            &accounts,
            &balances,
            &account_set_members,
            &clock,
        );
        let postings = Postings::new(
            &publisher,
            &tx_templates,
            &account_sets,
            &balances,
            &velocities,
        );

        let ec_rollup = crate::ec_rollup::register_ec_balance_rollup(
            jobs,
            publisher.inner(),
            &balances,
            &entries,
        )
        .await?;

        Ok(Self {
            ec_rollup,
            accounts,
            account_sets,
            postings,
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

    /// Post `tx_id` within a caller-supplied operation.
    ///
    /// The N=1 case of [`Self::post_transactions_in_op`] — same flow, same
    /// statements, arrays of length one.
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
        let transaction = self
            .postings
            .post_all_in_op(
                db,
                vec![PostingInput::new(tx_id, tx_template_code, params.into())],
            )
            .await?
            .pop()
            .expect("one posting in, one transaction out");

        let span = tracing::Span::current();
        span.record("transaction_id", transaction.id().to_string());
        span.record("external_id", &transaction.values().external_id);
        Ok(transaction)
    }

    /// Post many transactions in a single database transaction.
    ///
    /// Every phase of the flow is vectorised, so a batch costs the same number
    /// of round trips as a single posting — which is what makes batching worth
    /// doing: the per-commit WAL cost is amortised across the whole batch.
    ///
    /// **All-or-nothing.** Any failure aborts the batch and no posting lands;
    /// the error names the offending posting.
    ///
    /// **Ordering.** The result is exactly as if the postings had run one at a
    /// time, in the given order, inside one transaction: later postings observe
    /// earlier ones' balances, velocity limits enforce against the chained
    /// snapshots, and snapshot versions increment in order. Postings may span
    /// journals and templates freely.
    ///
    /// Concurrent batches are deadlock-free by construction: a batch takes one
    /// canonically sorted union lock batch over every posting's entry pairs,
    /// then one sorted ancestor batch — see [`crate::posting`].
    #[instrument(
        name = "cala_ledger.post_transactions",
        skip_all,
        fields(batch_size = batch.len())
    )]
    pub async fn post_transactions(
        &self,
        batch: Vec<PostingInput>,
    ) -> Result<Vec<Transaction>, LedgerError> {
        let mut db = es_entity::DbOp::init_with_clock(&self.pool, &self.clock).await?;
        let transactions = self.post_transactions_in_op(&mut db, batch).await?;
        db.commit().await?;
        Ok(transactions)
    }

    /// [`Self::post_transactions`] within a caller-supplied operation.
    ///
    /// Note that issuing several batches on one operation reintroduces lock
    /// acquisition across batch boundaries with no global ordering, which is
    /// exactly what a single batch avoids; prefer one call with all the
    /// postings.
    #[instrument(
        name = "cala_ledger.post_transactions_in_op",
        skip_all,
        fields(batch_size = batch.len())
    )]
    pub async fn post_transactions_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        batch: Vec<PostingInput>,
    ) -> Result<Vec<Transaction>, LedgerError> {
        Ok(self.postings.post_all_in_op(db, batch).await?)
    }

    /// Snapshot the rollup's position, pinning the outbox frontier as a
    /// fence. Cheap and read-only — poll [`lag`](crate::EcRollupStatus::lag)
    /// as a stream-lag SLO metric, or block on the fence with
    /// [`await_completion`](crate::EcRollupStatus::await_completion). Works
    /// from any node (both sides are read from the database).
    #[instrument(
        level = "debug",
        name = "cala_ledger.ec_rollup_status",
        skip_all,
        fields(applied, frontier, lag)
    )]
    pub async fn ec_rollup_status(&self) -> Result<crate::EcRollupStatus, LedgerError> {
        let snapshot = self.ec_rollup.load().await?;
        let status = crate::EcRollupStatus::new(snapshot.stream_status(), self.ec_rollup.clone());

        let span = tracing::Span::current();
        span.record("applied", u64::from(status.applied));
        span.record("frontier", u64::from(status.frontier));
        span.record("lag", status.lag());

        Ok(status)
    }

    /// Await the rollup applying through `frontier` — an outbox sequence
    /// obtained some other way than the snapshot this call is on, e.g. a
    /// [`EcRollupStatus::frontier`](crate::EcRollupStatus) read earlier and
    /// carried across a boundary the snapshot itself can't cross (stored,
    /// passed to another task, awaited from a different call site than the
    /// one that captured it).
    ///
    /// Unlike [`EcRollupStatus::await_completion`](crate::EcRollupStatus::await_completion),
    /// which is bound to the fence its own snapshot pinned, this takes any
    /// `frontier` value directly — the ledger itself is the only handle you
    /// need to hold onto. Prefer `await_completion` when the snapshot that
    /// pinned the fence is still in scope; reach for this when only the
    /// sequence survived.
    ///
    /// `timeout` is mandatory: a wedged rollup surfaces as
    /// [`LedgerError::EcCaughtUpTimeout`], never a silent hang.
    #[instrument(level = "debug", name = "cala_ledger.await_frontier", skip(self), fields(timeout = ?timeout))]
    pub async fn await_frontier(
        &self,
        frontier: obix::EventSequence,
        timeout: std::time::Duration,
    ) -> Result<(), LedgerError> {
        self.ec_rollup.await_sequence(frontier, timeout).await?;
        Ok(())
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
