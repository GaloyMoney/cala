//! The calculation rollup is a global outbox consumer, so every test here
//! runs against its own isolated database.
mod helpers;

use chrono::NaiveDate;
use futures::StreamExt;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use cala_ledger::{
    account::NewAccount,
    account_set::NewAccountSet,
    balance::{
        error::BalanceNotFound, AccountBalance, BalanceRange, EffectiveDates, EffectivePeriods,
        InvalidEffectiveDates, InvalidEffectivePeriods, PeriodGranularity,
        MAX_EFFECTIVE_PERIODS_WORK,
    },
    calculation::{
        error::{CalculationBackfillTimeout, CalculationBalanceNotFound},
        *,
    },
    errlanes::Fail,
    job::Jobs,
    journal::NewJournal,
    outbox::OutboxEventPayload,
    tx_template::*,
    *,
};

struct Fixture {
    cala: CalaLedger,
    jobs: Jobs,
    pool: sqlx::PgPool,
    journal_id: JournalId,
    sender: AccountId,
}

async fn setup(journal: NewJournal) -> anyhow::Result<Fixture> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool.clone())
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;
    let journal_id = cala.journals().create(journal).await?.id();
    let sender = create_account(&cala).await?;
    Ok(Fixture {
        cala,
        jobs,
        pool,
        journal_id,
        sender,
    })
}

fn random_code() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

async fn create_account(cala: &CalaLedger) -> anyhow::Result<AccountId> {
    let code = random_code();
    Ok(cala
        .accounts()
        .create(
            NewAccount::builder()
                .id(AccountId::new())
                .name(format!("Account {code}"))
                .code(code)
                .build(),
        )
        .await?
        .id())
}

async fn create_set(
    cala: &CalaLedger,
    journal_id: JournalId,
    rollup: BalanceRollup,
) -> anyhow::Result<AccountSetId> {
    Ok(cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name(format!("Set {}", random_code()))
                .journal_id(journal_id)
                .balance_rollup(rollup)
                .build(),
        )
        .await?
        .id())
}

/// Like `helpers::velocity_template`, with entry types `'{kind}_DR'` /
/// `'{kind}_CR'` and the entry metadata from `params.entry_meta`.
async fn create_template(cala: &CalaLedger, kind: &str) -> anyhow::Result<String> {
    let code = random_code();
    let param = |name: &str, ty: ParamDataType, default: Option<&str>| {
        let builder = NewParamDefinition::builder().name(name).r#type(ty);
        match default {
            Some(default) => builder.default_expr(default).build().unwrap(),
            None => builder.build().unwrap(),
        }
    };
    let params = vec![
        param("recipient", ParamDataType::Uuid, None),
        param("sender", ParamDataType::Uuid, None),
        param("journal_id", ParamDataType::Uuid, None),
        param("amount", ParamDataType::Decimal, None),
        param("currency", ParamDataType::String, Some("'USD'")),
        param("layer", ParamDataType::String, Some("'SETTLED'")),
        param("meta", ParamDataType::Json, Some(r#"{"foo": "bar"}"#)),
        param("entry_meta", ParamDataType::Json, Some("{}")),
        param("effective", ParamDataType::Date, Some("date()")),
    ];
    let entry = |side: &str, account: &str, direction: &str| {
        NewTxTemplateEntry::builder()
            .entry_type(format!("'{kind}_{side}'"))
            .account_id(account)
            .layer("params.layer")
            .direction(direction)
            .units("params.amount")
            .currency("params.currency")
            .metadata("params.entry_meta")
            .build()
            .unwrap()
    };
    cala.tx_templates()
        .create(
            NewTxTemplate::builder()
                .id(uuid::Uuid::now_v7())
                .code(&code)
                .params(params)
                .transaction(
                    NewTxTemplateTransaction::builder()
                        .effective("params.effective")
                        .journal_id("params.journal_id")
                        .metadata("params.meta")
                        .build()
                        .unwrap(),
                )
                .entries(vec![
                    entry("DR", "params.sender", "DEBIT"),
                    entry("CR", "params.recipient", "CREDIT"),
                ])
                .build(),
        )
        .await?;
    Ok(code)
}

struct Post<'a> {
    template: &'a str,
    sender: AccountId,
    recipient: AccountId,
    amount: i64,
    effective: NaiveDate,
    layer: &'static str,
    meta: serde_json::Value,
    entry_meta: serde_json::Value,
}

impl<'a> Post<'a> {
    fn new(
        template: &'a str,
        sender: AccountId,
        recipient: AccountId,
        amount: i64,
        effective: NaiveDate,
    ) -> Self {
        Self {
            template,
            sender,
            recipient,
            amount,
            effective,
            layer: "SETTLED",
            meta: json!({"foo": "bar"}),
            entry_meta: json!({}),
        }
    }
}

async fn post(f: &Fixture, p: Post<'_>) -> anyhow::Result<()> {
    let mut params = Params::new();
    params.insert("journal_id", f.journal_id);
    params.insert("sender", p.sender);
    params.insert("recipient", p.recipient);
    params.insert("amount", Decimal::from(p.amount));
    params.insert("effective", p.effective);
    params.insert("layer", p.layer);
    params.insert("meta", p.meta);
    params.insert("entry_meta", p.entry_meta);
    f.cala
        .post_transaction(TransactionId::new(), p.template, params)
        .await?;
    Ok(())
}

/// The tip caught up to the frontier and no backfill left open; the tip is
/// re-fenced afterwards because a handoff moves the frontier.
async fn wait_for_calculations(cala: &CalaLedger) -> anyhow::Result<()> {
    wait_for_tip(cala).await?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let open: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cala_calculation_backfills WHERE completed_at IS NULL",
            )
            .fetch_one(cala.pool())
            .await?;
            if open == 0 {
                return anyhow::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    wait_for_tip(cala).await
}

/// The tip alone: a unit in `InProgress` may still be open afterwards.
async fn wait_for_tip(cala: &CalaLedger) -> anyhow::Result<()> {
    cala.calculation_rollup_status()
        .await?
        .await_completion(Duration::from_secs(10))
        .await?;
    Ok(())
}

#[derive(Default)]
struct Config {
    condition: Option<&'static str>,
    dimensions: Vec<NewPartitionKey>,
    effective_date_source: Option<&'static str>,
    history: History,
}

async fn create_calculation(
    f: &Fixture,
    scope: CalculationScope,
    config: Config,
) -> anyhow::Result<CalculationId> {
    let new = NewCalculation::builder()
        .id(CalculationId::new())
        .journal_id(f.journal_id)
        .code(random_code())
        .name("rollup test")
        .scope(scope)
        .maybe_condition(config.condition)
        .dimensions(config.dimensions)
        .maybe_effective_date_source(config.effective_date_source)
        .history(config.history)
        .build()?;
    Ok(f.cala.calculations().create(new).await?.id())
}

fn id(
    f: &Fixture,
    calculation_id: CalculationId,
    account_id: impl Into<AccountId>,
    dimensions: serde_json::Value,
) -> CalculationBalanceId {
    CalculationBalanceId {
        journal_id: f.journal_id,
        calculation_id,
        account_id: account_id.into(),
        currency: "USD".parse().unwrap(),
        dimensions: Window::from(dimensions),
    }
}

type Row = (
    String,
    NaiveDate,
    Decimal,
    Decimal,
    Decimal,
    Decimal,
    Decimal,
    Decimal,
    i32,
    i32,
);

async fn base_rows(f: &Fixture, account_id: AccountId) -> anyhow::Result<Vec<Row>> {
    Ok(sqlx::query_as(
        "SELECT currency, effective, settled_dr_balance, settled_cr_balance,
                pending_dr_balance, pending_cr_balance,
                encumbrance_dr_balance, encumbrance_cr_balance, version, all_time_version
         FROM cala_cumulative_effective_balances
         WHERE journal_id = $1 AND account_id = $2
         ORDER BY currency, effective",
    )
    .bind(f.journal_id)
    .bind(account_id)
    .fetch_all(&f.pool)
    .await?)
}

async fn calculation_rows(
    f: &Fixture,
    calculation_id: CalculationId,
    account_id: AccountId,
    dimensions: serde_json::Value,
) -> anyhow::Result<Vec<Row>> {
    Ok(sqlx::query_as(
        "SELECT currency, effective, settled_dr_balance, settled_cr_balance,
                pending_dr_balance, pending_cr_balance,
                encumbrance_dr_balance, encumbrance_cr_balance, version, all_time_version
         FROM cala_calculation_effective_balances
         WHERE journal_id = $1 AND calculation_id = $2 AND account_id = $3
           AND dimensions = $4
         ORDER BY currency, effective",
    )
    .bind(f.journal_id)
    .bind(calculation_id)
    .bind(account_id)
    .bind(dimensions)
    .fetch_all(&f.pool)
    .await?)
}

/// Counts `tracing` spans and events under a span name: `flushes` spans
/// named `span`, `statements` `sqlx::query` events inside one, and `errors`
/// error events carrying a `calculation_id` field.
#[derive(Clone)]
struct Counter {
    span: &'static str,
    flushes: Arc<AtomicUsize>,
    statements: Arc<AtomicUsize>,
    shared_locks: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
}

impl Counter {
    fn new(span: &'static str) -> Self {
        Self {
            span,
            flushes: Arc::default(),
            statements: Arc::default(),
            shared_locks: Arc::default(),
            errors: Arc::default(),
        }
    }
}

/// Whether any recorded field of an event mentions `needle`.
struct Mentions(&'static str, bool);
impl tracing::field::Visit for Mentions {
    fn record_str(&mut self, _: &tracing::field::Field, value: &str) {
        self.1 |= value.contains(self.0);
    }
    fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.1 |= format!("{value:?}").contains(self.0);
    }
}

struct HasField(&'static str, bool);
impl tracing::field::Visit for HasField {
    fn record_debug(&mut self, field: &tracing::field::Field, _: &dyn std::fmt::Debug) {
        if field.name() == self.0 {
            self.1 = true;
        }
    }
}

impl<S> tracing_subscriber::Layer<S> for Counter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == self.span {
            self.flushes.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let in_span = ctx
            .event_scope(event)
            .is_some_and(|mut scope| scope.any(|span| span.name() == self.span));
        if !in_span {
            return;
        }
        if event.metadata().target() == "sqlx::query" {
            self.statements.fetch_add(1, Ordering::SeqCst);
            let mut visitor = Mentions("pg_advisory_xact_lock_shared", false);
            event.record(&mut visitor);
            if visitor.1 {
                self.shared_locks.fetch_add(1, Ordering::SeqCst);
            }
        }
        if *event.metadata().level() == tracing::Level::ERROR {
            let mut visitor = HasField("calculation_id", false);
            event.record(&mut visitor);
            if visitor.1 {
                self.errors.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn global_no_condition_equals_base_effective_series() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(&f, CalculationScope::Global, Config::default()).await?;

    let r1 = create_account(&f.cala).await?;
    let r2 = create_account(&f.cala).await?;
    let r3 = create_account(&f.cala).await?;
    let inner = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    let outer = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    let ec = create_set(&f.cala, f.journal_id, BalanceRollup::EventuallyConsistent).await?;
    let sets = f.cala.account_sets();
    sets.add_member(inner, r1).await?;
    sets.add_member(outer, inner).await?;
    sets.add_member(ec, r2).await?;
    sets.add_member(ec, r3).await?;

    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();
    let d = |n: i64| today - chrono::Duration::days(n);
    let s = f.sender;
    for p in [
        Post::new(&template, s, r1, 10, today),
        Post::new(&template, s, r2, 20, today),
        Post::new(&template, s, r3, 5, d(1)),
        Post::new(&template, s, r1, 7, d(3)),
        Post::new(&template, r2, r1, 3, today),
        Post {
            layer: "PENDING",
            ..Post::new(&template, s, r2, -4, d(2))
        },
        Post {
            layer: "ENCUMBRANCE",
            ..Post::new(&template, s, r3, 6, today)
        },
        Post::new(&template, s, r2, 2, d(5)),
    ] {
        post(&f, p).await?;
    }

    f.cala
        .ec_rollup_status()
        .await?
        .await_completion(Duration::from_secs(10))
        .await?;
    wait_for_calculations(&f.cala).await?;

    let targets: Vec<AccountId> = vec![s, r1, r2, r3, inner.into(), outer.into(), ec.into()];
    let effective = f.cala.balances().effective();
    let calc = f.cala.calculations().effective();
    let usd: Currency = "USD".parse()?;
    for target in targets {
        let base = base_rows(&f, target).await?;
        assert!(!base.is_empty(), "every target has base rows");
        assert_eq!(
            calculation_rows(&f, calculation, target, json!({})).await?,
            base,
            "calculation rows equal base rows for {target}"
        );
        for row in base.iter() {
            let date = row.1;
            let b = effective
                .find_cumulative(f.journal_id, target, usd, date)
                .await?;
            let c = calc
                .find_cumulative(&id(&f, calculation, target, json!({})), date)
                .await?;
            assert_eq!(b.details.settled.dr_balance, c.details.settled.dr_balance);
            assert_eq!(b.details.settled.cr_balance, c.details.settled.cr_balance);
            assert_eq!(b.details.pending.dr_balance, c.details.pending.dr_balance);
            assert_eq!(b.details.pending.cr_balance, c.details.pending.cr_balance);
            assert_eq!(
                b.details.encumbrance.dr_balance,
                c.details.encumbrance.dr_balance
            );
            assert_eq!(
                b.details.encumbrance.cr_balance,
                c.details.encumbrance.cr_balance
            );
            assert_eq!(b.details.version, c.details.version);
        }
        for (from, until) in [(d(4), Some(d(1))), (d(2), Some(today)), (d(10), None)] {
            let b = effective
                .find_in_range(f.journal_id, target, usd, from, until)
                .await?;
            let c = calc
                .find_in_range(&id(&f, calculation, target, json!({})), from, until)
                .await?;
            for (b, c) in [
                (&b.open, &c.open),
                (&b.period, &c.period),
                (&b.close, &c.close),
            ] {
                assert_eq!(b.settled(), c.settled());
                assert_eq!(b.pending(), c.pending());
                assert_eq!(b.encumbrance(), c.encumbrance());
                assert_eq!(b.details.version, c.details.version);
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn condition_excludes_entries() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(
        &f,
        CalculationScope::Global,
        Config {
            condition: Some(
                "!has(context.vars.transaction.metadata) || !has(context.vars.transaction.metadata.is_closing_tx) || !context.vars.transaction.metadata.is_closing_tx",
            ),
            ..Config::default()
        },
    )
    .await?;
    let revenue = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;

    for month in 1..=12 {
        post(
            &f,
            Post::new(&template, f.sender, revenue, 10, day(2025, month, 15)),
        )
        .await?;
    }
    post(
        &f,
        Post {
            meta: json!({"is_closing_tx": true}),
            ..Post::new(&template, revenue, f.sender, 120, day(2025, 12, 31))
        },
    )
    .await?;
    wait_for_calculations(&f.cala).await?;

    let usd: Currency = "USD".parse()?;
    let base = f
        .cala
        .balances()
        .effective()
        .find_in_range(
            f.journal_id,
            revenue,
            usd,
            day(2025, 12, 1),
            Some(day(2025, 12, 31)),
        )
        .await?;
    assert_eq!(base.period.settled(), Decimal::from(10 - 120));
    let id = id(&f, calculation, revenue, json!({}));
    let calc = f.cala.calculations().effective();
    let range = calc
        .find_in_range(&id, day(2025, 12, 1), Some(day(2025, 12, 31)))
        .await?;
    assert_eq!(range.period.settled(), Decimal::from(10));
    let year_end = calc.find_cumulative(&id, day(2025, 12, 31)).await?;
    assert_eq!(year_end.settled(), Decimal::from(120));
    Ok(())
}

#[tokio::test]
async fn dimensions_split_series() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let test = create_template(&f.cala, "TEST").await?;
    let other = create_template(&f.cala, "OTHER").await?;
    let calculation = create_calculation(
        &f,
        CalculationScope::Global,
        Config {
            dimensions: vec![NewPartitionKey::builder()
                .alias("entry_type")
                .value("context.vars.entry.entryType")
                .build()?],
            ..Config::default()
        },
    )
    .await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;

    let today = f.cala.clock().now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    post(&f, Post::new(&test, f.sender, recipient, 10, yesterday)).await?;
    post(&f, Post::new(&other, f.sender, recipient, 3, yesterday)).await?;
    post(&f, Post::new(&test, f.sender, recipient, 4, today)).await?;
    post(&f, Post::new(&other, f.sender, recipient, 8, today)).await?;
    wait_for_calculations(&f.cala).await?;

    let usd: Currency = "USD".parse()?;
    let calc = f.cala.calculations().effective();
    for date in [yesterday, today] {
        let base = f
            .cala
            .balances()
            .effective()
            .find_cumulative(f.journal_id, recipient, usd, date)
            .await?;
        let test_series = calc
            .find_cumulative(
                &id(&f, calculation, recipient, json!({"entry_type": "TEST_CR"})),
                date,
            )
            .await?;
        let other_series = calc
            .find_cumulative(
                &id(
                    &f,
                    calculation,
                    recipient,
                    json!({"entry_type": "OTHER_CR"}),
                ),
                date,
            )
            .await?;
        assert_eq!(
            test_series.settled() + other_series.settled(),
            base.settled()
        );
    }

    let listed = calc
        .list_cumulative_for_account(
            f.journal_id,
            calculation,
            recipient,
            today,
            es_entity::PaginatedQueryArgs {
                first: 10,
                after: None,
            },
        )
        .await?;
    let tuples: Vec<(serde_json::Value, Decimal)> = listed
        .entities()
        .iter()
        .map(|(window, balance)| (window.inner().clone(), balance.settled()))
        .collect();
    assert_eq!(
        tuples,
        vec![
            (json!({"entry_type": "OTHER_CR"}), Decimal::from(11)),
            (json!({"entry_type": "TEST_CR"}), Decimal::from(14)),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn attached_scope_covers_only_targets() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let r1 = create_account(&f.cala).await?;
    let r2 = create_account(&f.cala).await?;
    let inner = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    let outer = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    f.cala.account_sets().add_member(inner, r1).await?;
    f.cala.account_sets().add_member(outer, inner).await?;
    let calculation = create_calculation(&f, CalculationScope::Attached, Config::default()).await?;
    f.cala.calculations().attach(calculation, inner).await?;
    f.cala.calculations().attach(calculation, r2).await?;
    f.jobs.start_poll().await?;

    let today = f.cala.clock().now().date_naive();
    post(&f, Post::new(&template, f.sender, r1, 10, today)).await?;
    post(&f, Post::new(&template, f.sender, r2, 4, today)).await?;
    wait_for_calculations(&f.cala).await?;

    let calc = f.cala.calculations().effective();
    let covered = calc
        .find_cumulative(&id(&f, calculation, inner, json!({})), today)
        .await?;
    assert_eq!(covered.settled(), Decimal::from(10));
    let covered = calc
        .find_cumulative(&id(&f, calculation, r2, json!({})), today)
        .await?;
    assert_eq!(covered.settled(), Decimal::from(4));
    for uncovered in [r1, outer.into(), f.sender] {
        assert!(matches!(
            calc.find_cumulative(&id(&f, calculation, uncovered, json!({})), today)
                .await,
            Err(Fail::Rejected(_))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn activation_boundary() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let r1 = create_account(&f.cala).await?;
    let r2 = create_account(&f.cala).await?;
    let attached =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();

    post(&f, Post::new(&template, f.sender, r1, 1, today)).await?;
    let global =
        create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    post(&f, Post::new(&template, f.sender, r1, 10, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let rows = calculation_rows(&f, global, r1, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, Decimal::from(10), "only B's credit");
    assert_eq!(rows[0].9, 1, "all_time_version counts B's entry only");

    post(&f, Post::new(&template, f.sender, r2, 100, today)).await?;
    attach_many_with_history(&f, attached, &[r2], History::FromNow).await?;
    post(&f, Post::new(&template, f.sender, r2, 1000, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let rows = calculation_rows(&f, attached, r2, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, Decimal::from(1000), "only D's credit");
    assert_eq!(rows[0].9, 1);
    Ok(())
}

#[tokio::test]
async fn effective_date_source() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(
        &f,
        CalculationScope::Global,
        Config {
            effective_date_source: Some("date(context.vars.entry.metadata.statement_date)"),
            ..Config::default()
        },
    )
    .await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;

    let today = f.cala.clock().now().date_naive();
    let statement_date = day(2025, 3, 7);
    post(
        &f,
        Post {
            entry_meta: json!({"statement_date": "2025-03-07"}),
            ..Post::new(&template, f.sender, recipient, 9, today)
        },
    )
    .await?;
    wait_for_calculations(&f.cala).await?;

    let rows = calculation_rows(&f, calculation, recipient, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, statement_date);
    assert_eq!(rows[0].3, Decimal::from(9));
    let base = base_rows(&f, recipient).await?;
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].1, today);
    Ok(())
}

#[tokio::test]
async fn evaluation_error_is_skipped_not_fatal() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    let counter = Counter::new("cala_ledger.calculation_rollup.flush");
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(counter.clone()))
        .expect("no other global tracing subscriber should be set in this test process");

    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(
        &f,
        CalculationScope::Global,
        Config {
            condition: Some("context.vars.entry.metadata.missing == 1"),
            history: History::FromNow,
            ..Config::default()
        },
    )
    .await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;

    let today = f.cala.clock().now().date_naive();
    post(&f, Post::new(&template, f.sender, recipient, 1, today)).await?;
    post(&f, Post::new(&template, f.sender, recipient, 2, today)).await?;
    wait_for_calculations(&f.cala).await?;

    for account in [recipient, f.sender] {
        assert!(calculation_rows(&f, calculation, account, json!({}))
            .await?
            .is_empty());
    }
    assert_eq!(
        counter.errors.load(Ordering::SeqCst),
        4,
        "one error per (entry, covered target)"
    );
    Ok(())
}

#[tokio::test]
async fn no_calculations_costs_two_statements() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    let calculation = Counter::new("cala_ledger.calculation_rollup.flush");
    let ec = Counter::new("cala_ledger.ec_rollup.flush");
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(calculation.clone())
            .with(ec.clone()),
    )
    .expect("no other global tracing subscriber should be set in this test process");

    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();
    for amount in 1..=3 {
        post(&f, Post::new(&template, f.sender, recipient, amount, today)).await?;
    }
    wait_for_calculations(&f.cala).await?;
    f.cala
        .ec_rollup_status()
        .await?
        .await_completion(Duration::from_secs(10))
        .await?;

    let flushes = calculation.flushes.load(Ordering::SeqCst);
    let statements = calculation.statements.load(Ordering::SeqCst);
    assert!(flushes >= 1, "the calculation rollup flushed");
    assert_eq!(
        statements,
        2 * flushes,
        "one shared handoff lock and one EXISTS probe per flush without calculations"
    );
    assert!(
        ec.statements.load(Ordering::SeqCst) > ec.flushes.load(Ordering::SeqCst),
        "positive control: statements are observed inside a flush span"
    );
    Ok(())
}

/// Blocks the calculation rollup's first statement until the returned
/// transaction ends.
async fn stall_calculation_rollup(
    pool: &sqlx::PgPool,
) -> anyhow::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE cala_calculations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

#[tokio::test]
async fn ec_rollup_lag_independent() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    create_calculation(&f, CalculationScope::Global, Config::default()).await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    wait_for_calculations(&f.cala).await?;

    let stall = stall_calculation_rollup(&f.pool).await?;
    let today = f.cala.clock().now().date_naive();
    post(&f, Post::new(&template, f.sender, recipient, 5, today)).await?;

    f.cala
        .ec_rollup_status()
        .await?
        .await_completion(Duration::from_secs(10))
        .await?;
    let status = f.cala.calculation_rollup_status().await?;
    assert!(status.lag() > 0);
    assert!(!status.is_caught_up());

    stall.rollback().await?;
    wait_for_calculations(&f.cala).await?;
    Ok(())
}

#[tokio::test]
async fn status_and_fence() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    create_calculation(&f, CalculationScope::Global, Config::default()).await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();

    post(&f, Post::new(&template, f.sender, recipient, 1, today)).await?;
    let mut status = f.cala.calculation_rollup_status().await?;
    status.await_completion(Duration::from_secs(10)).await?;
    status.refresh().await?;
    assert!(status.is_caught_up());
    assert_eq!(status.lag(), 0);

    let stall = stall_calculation_rollup(&f.pool).await?;
    post(&f, Post::new(&template, f.sender, recipient, 2, today)).await?;
    let frontier = f.cala.calculation_rollup_status().await?.frontier;
    assert!(matches!(
        f.cala
            .await_calculation_frontier(frontier, Duration::from_millis(1))
            .await,
        Err(Fail::Rejected(EcCaughtUpTimeout { .. }))
    ));

    stall.rollback().await?;
    f.cala
        .await_calculation_frontier(frontier, Duration::from_secs(10))
        .await?;
    Ok(())
}

fn assert_amounts_eq(actual: &AccountBalance, expected: &AccountBalance, what: &str) {
    assert_eq!(actual.settled(), expected.settled(), "{what} settled");
    assert_eq!(actual.pending(), expected.pending(), "{what} pending");
    assert_eq!(
        actual.encumbrance(),
        expected.encumbrance(),
        "{what} encumbrance"
    );
}

#[tokio::test]
async fn periods_match_range_and_cumulative() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(&f, CalculationScope::Global, Config::default()).await?;
    let recipient = create_account(&f.cala).await?;
    let set = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    f.cala.account_sets().add_member(set, recipient).await?;
    f.jobs.start_poll().await?;

    let s = f.sender;
    for p in [
        Post::new(&template, s, recipient, 10, day(2024, 1, 10)),
        Post::new(&template, s, recipient, 20, day(2024, 3, 20)),
        Post {
            layer: "PENDING",
            ..Post::new(&template, s, recipient, 7, day(2024, 2, 5))
        },
        Post::new(&template, recipient, s, 4, day(2024, 1, 25)),
        Post::new(&template, s, recipient, 1, day(2024, 3, 31)),
    ] {
        post(&f, p).await?;
    }
    wait_for_calculations(&f.cala).await?;

    let periods =
        EffectivePeriods::try_from_count(PeriodGranularity::Month, day(2023, 12, 1), 5).unwrap();
    let usd: Currency = "USD".parse()?;
    let targets: [AccountId; 3] = [s, recipient, set.into()];
    let ids: Vec<CalculationBalanceId> = targets
        .iter()
        .map(|t| id(&f, calculation, *t, json!({})))
        .collect();
    let calc = f.cala.calculations().effective();
    let series = calc.find_all_periods(&ids, &periods).await?;
    let base_ids: Vec<BalanceId> = targets.iter().map(|t| (f.journal_id, *t, usd)).collect();
    let base_series = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&base_ids, &periods)
        .await?;
    assert_eq!(series.len(), 3);

    for (id, base_id) in ids.iter().zip(&base_ids) {
        let series = &series[id];
        let base = &base_series[base_id];
        assert_eq!(series.periods.len(), periods.len());
        assert_eq!(
            calc.find_periods(id, &periods).await?.periods.len(),
            periods.len()
        );
        for ((period, base), (start, end)) in series
            .periods
            .iter()
            .zip(&base.periods)
            .zip(periods.bounds())
        {
            assert_eq!((period.start, period.end), (start, end));
            let what = format!("{} {start}..{end}", id.account_id);
            assert_amounts_eq(&period.range.open, &base.range.open, &what);
            assert_amounts_eq(&period.range.period, &base.range.period, &what);
            assert_amounts_eq(&period.range.close, &base.range.close, &what);
            assert_eq!(
                period.range.period.details.version, base.range.period.details.version,
                "{what} version vs base"
            );

            if start < day(2024, 1, 1) {
                assert_eq!(period.range.close.settled(), Decimal::ZERO);
                continue;
            }
            let cumulative = calc.find_cumulative(id, end).await?;
            assert_amounts_eq(&period.range.close, &cumulative, &what);
            let range = calc.find_in_range(id, start, Some(end)).await?;
            assert_amounts_eq(&period.range.open, &range.open, &what);
            assert_amounts_eq(&period.range.period, &range.period, &what);
            assert_eq!(
                period.range.period.details.version, range.period.details.version,
                "{what} version vs range"
            );
        }
    }

    let absent = CalculationBalanceId {
        account_id: AccountId::new(),
        ..ids[0].clone()
    };
    let with_absent = [ids[0].clone(), absent.clone()];
    let partial = calc.find_all_periods(&with_absent, &periods).await?;
    assert_eq!(partial.len(), 1);
    assert!(matches!(
        calc.find_periods(&absent, &periods).await,
        Err(Fail::Rejected(CalculationBalanceNotFound(_)))
    ));

    let too_many = vec![ids[0].clone(); MAX_EFFECTIVE_PERIODS_WORK / (periods.len() + 1) + 1];
    assert!(matches!(
        calc.find_all_periods(&too_many, &periods).await,
        Err(Fail::Rejected(InvalidEffectivePeriods))
    ));
    Ok(())
}

/// The postings of `periods_match_range_and_cumulative` under one Global
/// calculation, rolled up. Returns the calculation series ids and the base
/// ids of `[sender, recipient, set]`.
async fn cumulative_fixture() -> anyhow::Result<(Fixture, Vec<CalculationBalanceId>, Vec<BalanceId>)>
{
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let calculation = create_calculation(&f, CalculationScope::Global, Config::default()).await?;
    let recipient = create_account(&f.cala).await?;
    let set = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    f.cala.account_sets().add_member(set, recipient).await?;
    f.jobs.start_poll().await?;

    let s = f.sender;
    for p in [
        Post::new(&template, s, recipient, 10, day(2024, 1, 10)),
        Post::new(&template, s, recipient, 20, day(2024, 3, 20)),
        Post {
            layer: "PENDING",
            ..Post::new(&template, s, recipient, 7, day(2024, 2, 5))
        },
        Post::new(&template, recipient, s, 4, day(2024, 1, 25)),
        Post::new(&template, s, recipient, 1, day(2024, 3, 31)),
    ] {
        post(&f, p).await?;
    }
    wait_for_calculations(&f.cala).await?;

    let usd: Currency = "USD".parse()?;
    let targets: [AccountId; 3] = [s, recipient, set.into()];
    let ids = targets
        .iter()
        .map(|t| id(&f, calculation, *t, json!({})))
        .collect();
    let base_ids = targets.iter().map(|t| (f.journal_id, *t, usd)).collect();
    Ok((f, ids, base_ids))
}

fn assert_range_amounts_eq(actual: &BalanceRange, expected: &BalanceRange, what: &str) {
    assert_amounts_eq(&actual.open, &expected.open, &format!("{what} open"));
    assert_amounts_eq(&actual.period, &expected.period, &format!("{what} period"));
    assert_amounts_eq(&actual.close, &expected.close, &format!("{what} close"));
    assert_eq!(
        actual.period.details.version, expected.period.details.version,
        "{what} version"
    );
}

#[tokio::test]
async fn cumulative_at_matches_base_and_own_reads() -> anyhow::Result<()> {
    let (f, ids, base_ids) = cumulative_fixture().await?;
    let dates = EffectiveDates::try_new([
        day(2023, 12, 31),
        day(2024, 1, 17),
        day(2024, 2, 29),
        day(2024, 3, 17),
        day(2024, 3, 31),
    ])
    .unwrap();
    let ranges = [
        (day(2024, 1, 1), day(2024, 3, 17)),
        (day(2024, 1, 18), day(2024, 2, 29)),
        (day(2024, 3, 1), day(2024, 3, 17)),
        (day(2024, 1, 1), day(2024, 3, 31)),
    ];

    let calc = f.cala.calculations().effective();
    let cumulative = calc.find_all_cumulative_at(&ids, &dates).await?;
    let base_cumulative = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&base_ids, &dates)
        .await?;
    assert_eq!(cumulative.len(), 3);

    for (id, base_id) in ids.iter().zip(&base_ids) {
        let cumulative = &cumulative[id];
        let base = &base_cumulative[base_id];
        let single = calc.find_cumulative_at(id, &dates).await?;
        assert_eq!(cumulative.iter().count(), dates.len());
        for &d in dates.dates() {
            let what = format!("{} at {d}", id.account_id);
            let at = cumulative.at(d).expect("requested date");
            assert_amounts_eq(at, base.at(d).unwrap(), &what);
            assert_eq!(at.details.version, base.at(d).unwrap().details.version);
            assert_eq!(at.details, single.at(d).unwrap().details, "{what} single");
            match calc.find_cumulative(id, d).await {
                Ok(own) => {
                    assert_amounts_eq(at, &own, &what);
                    assert_eq!(at.details.version, own.details.version, "{what}");
                }
                Err(Fail::Rejected(CalculationBalanceNotFound(_))) => {
                    assert_eq!(at.settled(), Decimal::ZERO, "{what}");
                    assert_eq!(at.details.version, 0, "{what}");
                }
                Err(e) => return Err(e.into()),
            }
        }
        for (from, until) in ranges {
            let what = format!("{} {from}..{until}", id.account_id);
            let range = cumulative.range(from, until).expect("requested bounds");
            assert_range_amounts_eq(&range, &base.range(from, until).unwrap(), &what);
            let own = calc.find_in_range(id, from, Some(until)).await?;
            assert_range_amounts_eq(&range, &own, &what);
        }
    }

    let absent = CalculationBalanceId {
        account_id: AccountId::new(),
        ..ids[0].clone()
    };
    let with_absent = [ids[0].clone(), absent.clone()];
    let partial = calc.find_all_cumulative_at(&with_absent, &dates).await?;
    assert_eq!(partial.len(), 1);
    assert!(matches!(
        calc.find_cumulative_at(&absent, &dates).await,
        Err(Fail::Rejected(CalculationBalanceNotFound(_)))
    ));
    assert!(matches!(
        f.cala
            .balances()
            .effective()
            .find_cumulative_at(f.journal_id, absent.account_id, absent.currency, &dates)
            .await,
        Err(Fail::Rejected(BalanceNotFound(..)))
    ));

    let too_many = vec![ids[0].clone(); MAX_EFFECTIVE_PERIODS_WORK / dates.len() + 1];
    assert!(matches!(
        calc.find_all_cumulative_at(&too_many, &dates).await,
        Err(Fail::Rejected(InvalidEffectiveDates))
    ));
    Ok(())
}

#[tokio::test]
async fn calculation_periods_equals_cumulative_projection() -> anyhow::Result<()> {
    let (f, ids, _) = cumulative_fixture().await?;
    let periods =
        EffectivePeriods::try_from_count(PeriodGranularity::Month, day(2023, 12, 1), 5).unwrap();
    let dates = EffectiveDates::try_new(
        std::iter::once(day(2023, 11, 30)).chain(periods.bounds().map(|(_, end)| end)),
    )
    .unwrap();

    let calc = f.cala.calculations().effective();
    let series = calc.find_all_periods(&ids, &periods).await?;
    let cumulative = calc.find_all_cumulative_at(&ids, &dates).await?;
    assert_eq!(series.len(), 3);
    assert_eq!(cumulative.len(), 3);
    for id in &ids {
        let series = &series[id];
        let cumulative = &cumulative[id];
        assert_eq!(series.periods.len(), periods.len());
        for (period, (start, end)) in series.periods.iter().zip(periods.bounds()) {
            assert_eq!((period.start, period.end), (start, end));
            let projected = cumulative.range(start, end).expect("period bounds");
            assert_eq!(period.range.open.details, projected.open.details);
            assert_eq!(period.range.period.details, projected.period.details);
            assert_eq!(period.range.close.details, projected.close.details);
        }
    }
    Ok(())
}

async fn create_calculation_with_history(
    f: &Fixture,
    scope: CalculationScope,
    history: History,
) -> anyhow::Result<CalculationId> {
    let new = NewCalculation::builder()
        .id(CalculationId::new())
        .journal_id(f.journal_id)
        .code(random_code())
        .name("history test")
        .scope(scope)
        .history(history)
        .build()?;
    Ok(f.cala.calculations().create(new).await?.id())
}

async fn attach_many_with_history(
    f: &Fixture,
    calculation_id: CalculationId,
    targets: &[AccountId],
    history: History,
) -> anyhow::Result<()> {
    let mut op = f.cala.begin_operation().await?;
    f.cala
        .calculations()
        .attach_many_with_history_in_op(&mut op, calculation_id, targets, history)
        .await?;
    op.commit().await?;
    Ok(())
}

async fn attachment_backfill_complete(
    f: &Fixture,
    calculation_id: CalculationId,
    account_id: AccountId,
) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT backfill_complete FROM cala_calculation_attachments
         WHERE calculation_id = $1 AND account_id = $2",
    )
    .bind(calculation_id)
    .bind(account_id)
    .fetch_one(&f.pool)
    .await?)
}

#[tokio::test]
async fn from_now_matches_previous_behaviour() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let r1 = create_account(&f.cala).await?;
    let r2 = create_account(&f.cala).await?;
    let attached =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();

    post(&f, Post::new(&template, f.sender, r1, 1, today)).await?;
    let global =
        create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    assert_eq!(
        f.cala
            .calculations()
            .find_by_id(global)
            .await?
            .values()
            .backfill_status,
        BackfillStatus::Complete
    );
    post(&f, Post::new(&template, f.sender, r1, 10, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let rows = calculation_rows(&f, global, r1, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, Decimal::from(10), "only B's credit");
    assert_eq!(rows[0].9, 1, "all_time_version counts B's entry only");

    post(&f, Post::new(&template, f.sender, r2, 100, today)).await?;
    attach_many_with_history(&f, attached, &[r2], History::FromNow).await?;
    assert!(attachment_backfill_complete(&f, attached, r2).await?);
    post(&f, Post::new(&template, f.sender, r2, 1000, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let rows = calculation_rows(&f, attached, r2, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, Decimal::from(1000), "only D's credit");
    assert_eq!(rows[0].9, 1);
    assert!(f
        .cala
        .calculations()
        .list_open_backfills(attached)
        .await?
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn in_progress_unit_is_invisible_to_tip() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let recipient = create_account(&f.cala).await?;
    let calculation =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    let open = f
        .cala
        .calculations()
        .list_open_backfills(calculation)
        .await?;
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].account_ids, None);
    assert_eq!(open[0].cursor, 0);

    let stall = stall_backfill_jobs(&f.pool).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();
    post(&f, Post::new(&template, f.sender, recipient, 5, today)).await?;
    wait_for_tip(&f.cala).await?;

    assert_eq!(
        f.cala
            .calculations()
            .find_by_id(calculation)
            .await?
            .values()
            .backfill_status,
        BackfillStatus::InProgress
    );
    for account in [recipient, f.sender] {
        assert!(calculation_rows(&f, calculation, account, json!({}))
            .await?
            .is_empty());
    }
    assert!(matches!(
        f.cala
            .calculations()
            .effective()
            .find_cumulative(&id(&f, calculation, recipient, json!({})), today)
            .await,
        Err(Fail::Rejected(CalculationBalanceNotFound(_)))
    ));

    stall.rollback().await?;
    wait_for_backfill(&f.cala, calculation, None).await?;
    assert!(!calculation_rows(&f, calculation, recipient, json!({}))
        .await?
        .is_empty());
    Ok(())
}

/// Parks every backfill job at its first statement until the returned
/// transaction ends. The tip never reads this table, so it runs freely.
async fn stall_backfill_jobs(
    pool: &sqlx::PgPool,
) -> anyhow::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE cala_calculation_backfills IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

#[tokio::test]
async fn attach_partition() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let fresh = create_account(&f.cala).await?;
    let with_entry = create_account(&f.cala).await?;
    let set: AccountId = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous)
        .await?
        .into();
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();
    post(&f, Post::new(&template, f.sender, with_entry, 1, today)).await?;

    let calculation =
        create_calculation_with_history(&f, CalculationScope::Attached, History::Backfill).await?;
    attach_many_with_history(
        &f,
        calculation,
        &[fresh, with_entry, set],
        History::Backfill,
    )
    .await?;

    assert!(attachment_backfill_complete(&f, calculation, fresh).await?);
    assert!(!attachment_backfill_complete(&f, calculation, with_entry).await?);
    assert!(!attachment_backfill_complete(&f, calculation, set).await?);
    let open = f
        .cala
        .calculations()
        .list_open_backfills(calculation)
        .await?;
    assert_eq!(open.len(), 1, "one backfill row for the whole call");
    let ids: std::collections::HashSet<AccountId> =
        open[0].account_ids.clone().unwrap().into_iter().collect();
    assert_eq!(ids, [with_entry, set].into_iter().collect());
    Ok(())
}

#[tokio::test]
async fn tip_takes_shared_lock_on_every_flush() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    let calculation = Counter::new("cala_ledger.calculation_rollup.flush");
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(calculation.clone()),
    )
    .expect("no other global tracing subscriber should be set in this test process");

    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();

    post(&f, Post::new(&template, f.sender, recipient, 1, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let flushes = calculation.flushes.load(Ordering::SeqCst);
    assert!(flushes >= 1, "the calculation rollup flushed");
    assert_eq!(
        calculation.shared_locks.load(Ordering::SeqCst),
        flushes,
        "one shared handoff lock per flush before any calculation exists"
    );
    assert!(
        calculation.statements.load(Ordering::SeqCst) > flushes,
        "positive control: statements are observed inside a flush span"
    );

    create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    post(&f, Post::new(&template, f.sender, recipient, 2, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let flushes = calculation.flushes.load(Ordering::SeqCst);
    assert_eq!(
        calculation.shared_locks.load(Ordering::SeqCst),
        flushes,
        "one shared handoff lock per flush with a Complete unit"
    );

    create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    post(&f, Post::new(&template, f.sender, recipient, 3, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let flushes = calculation.flushes.load(Ordering::SeqCst);
    assert_eq!(
        calculation.shared_locks.load(Ordering::SeqCst),
        flushes,
        "one shared handoff lock per flush with a backfill open"
    );
    Ok(())
}

// ---- Backfill (handoff §10.B) ----

const HANDOFF_LOCK_CLASS: i32 = 6;

async fn wait_for_backfill(
    cala: &CalaLedger,
    id: CalculationId,
    target: Option<AccountId>,
) -> anyhow::Result<()> {
    cala.calculations()
        .await_backfill(id, target, Duration::from_secs(30))
        .await?;
    Ok(())
}

/// Holds the EXCLUSIVE side of the calculation handoff lock until the
/// returned transaction ends: every tip flush blocks at its first
/// statement and no handoff can commit, while posting, creating units and
/// the job's chase proceed.
async fn hold_handoff_lock(
    pool: &sqlx::PgPool,
) -> anyhow::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1::int4, 0)")
        .bind(HANDOFF_LOCK_CLASS)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Every series row of `calculation_id` for `account_id`, in every column
/// but the calculation id and the timestamps, which are per fold landing
/// (`modified_at` is the batch's latest `created_at`).
async fn series(
    f: &Fixture,
    calculation_id: CalculationId,
    account_id: AccountId,
) -> anyhow::Result<Vec<serde_json::Value>> {
    Ok(sqlx::query_scalar(
        "SELECT to_jsonb(t) - 'calculation_id' - 'created_at' - 'updated_at'
                - 'settled_modified_at' - 'pending_modified_at' - 'encumbrance_modified_at'
         FROM cala_calculation_effective_balances t
         WHERE journal_id = $1 AND calculation_id = $2 AND account_id = $3
         ORDER BY dimensions::text, currency, effective",
    )
    .bind(f.journal_id)
    .bind(calculation_id)
    .bind(account_id)
    .fetch_all(&f.pool)
    .await?)
}

async fn assert_same_series(
    f: &Fixture,
    oracle: CalculationId,
    backfilled: CalculationId,
    targets: &[AccountId],
) -> anyhow::Result<()> {
    for target in targets {
        let expected = series(f, oracle, *target).await?;
        assert!(!expected.is_empty(), "the oracle has rows for {target}");
        assert_eq!(
            series(f, backfilled, *target).await?,
            expected,
            "series of {target}"
        );
    }
    Ok(())
}

async fn backfill_status(f: &Fixture, id: CalculationId) -> anyhow::Result<BackfillStatus> {
    Ok(f.cala
        .calculations()
        .find_by_id(id)
        .await?
        .values()
        .backfill_status)
}

async fn activation_sequence(f: &Fixture, id: CalculationId) -> anyhow::Result<u64> {
    Ok(f.cala
        .calculations()
        .find_by_id(id)
        .await?
        .values()
        .activation_sequence)
}

async fn open_cursor(f: &Fixture, id: CalculationId) -> anyhow::Result<Option<u64>> {
    Ok(f.cala
        .calculations()
        .list_open_backfills(id)
        .await?
        .first()
        .map(|b| b.cursor))
}

/// Poll until the open backfill of `id` has `cursor == target`.
async fn wait_for_cursor(f: &Fixture, id: CalculationId, target: u64) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if open_cursor(f, id).await? == Some(target) {
                return anyhow::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

async fn checkpoint(f: &Fixture) -> anyhow::Result<u64> {
    Ok(u64::from(f.cala.calculation_rollup_status().await?.applied))
}

async fn wait_for_ec(f: &Fixture) -> anyhow::Result<()> {
    f.cala
        .ec_rollup_status()
        .await?
        .await_completion(Duration::from_secs(10))
        .await?;
    Ok(())
}

fn posting_input(
    f: &Fixture,
    template: &str,
    sender: AccountId,
    recipient: AccountId,
    amount: i64,
    effective: NaiveDate,
) -> posting::PostingInput {
    let mut params = Params::new();
    params.insert("journal_id", f.journal_id);
    params.insert("sender", sender);
    params.insert("recipient", recipient);
    params.insert("amount", Decimal::from(amount));
    params.insert("effective", effective);
    posting::PostingInput::new(TransactionId::new(), template, params)
}

/// `count` two-entry postings from the sender to `recipient`, in batches
/// of 50.
async fn post_many(
    f: &Fixture,
    template: &str,
    recipient: AccountId,
    count: usize,
) -> anyhow::Result<()> {
    let today = f.cala.clock().now().date_naive();
    let inputs: Vec<_> = (0..count)
        .map(|i| posting_input(f, template, f.sender, recipient, i as i64 + 1, today))
        .collect();
    for batch in inputs.chunks(50) {
        f.cala.post_transactions(batch.to_vec()).await?;
    }
    Ok(())
}

/// Three accounts under a synchronous set chain and an EC set.
struct HistoryAccounts {
    r1: AccountId,
    r2: AccountId,
    r3: AccountId,
    inner: AccountSetId,
    outer: AccountSetId,
    ec: AccountSetId,
}

impl HistoryAccounts {
    fn targets(&self, sender: AccountId) -> Vec<AccountId> {
        vec![
            sender,
            self.r1,
            self.r2,
            self.r3,
            self.inner.into(),
            self.outer.into(),
            self.ec.into(),
        ]
    }
}

async fn history_accounts(f: &Fixture) -> anyhow::Result<HistoryAccounts> {
    let r1 = create_account(&f.cala).await?;
    let r2 = create_account(&f.cala).await?;
    let r3 = create_account(&f.cala).await?;
    let inner = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    let outer = create_set(&f.cala, f.journal_id, BalanceRollup::Synchronous).await?;
    let ec = create_set(&f.cala, f.journal_id, BalanceRollup::EventuallyConsistent).await?;
    let sets = f.cala.account_sets();
    sets.add_member(inner, r1).await?;
    sets.add_member(outer, inner).await?;
    sets.add_member(ec, r2).await?;
    sets.add_member(ec, r3).await?;
    Ok(HistoryAccounts {
        r1,
        r2,
        r3,
        inner,
        outer,
        ec,
    })
}

/// 31 postings (62 entries) over the history accounts, backdated up to
/// five days, on all three layers. `round` shifts the amounts so repeated
/// rounds stay distinguishable.
async fn post_history(
    f: &Fixture,
    template: &str,
    h: &HistoryAccounts,
    round: i64,
) -> anyhow::Result<()> {
    let today = f.cala.clock().now().date_naive();
    let d = |n: i64| today - chrono::Duration::days(n);
    let s = f.sender;
    for i in 0..10 {
        let base = round * 100 + i;
        post(f, Post::new(template, s, h.r1, base + 1, d(i % 4))).await?;
        post(
            f,
            Post {
                layer: "PENDING",
                ..Post::new(template, s, h.r2, base + 2, d(i % 3))
            },
        )
        .await?;
        post(f, Post::new(template, h.r2, h.r3, base + 3, d(i % 6))).await?;
    }
    post(
        f,
        Post {
            layer: "ENCUMBRANCE",
            ..Post::new(template, s, h.r3, round + 7, d(2))
        },
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn backfilled_global_equals_preexisting_global() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let a = create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    let h = history_accounts(&f).await?;
    let targets = h.targets(f.sender);
    f.jobs.start_poll().await?;

    post_history(&f, &template, &h, 0).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;

    let b =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    wait_for_backfill(&f.cala, b, None).await?;
    assert_eq!(backfill_status(&f, b).await?, BackfillStatus::Complete);
    assert!(f
        .cala
        .calculations()
        .list_open_backfills(b)
        .await?
        .is_empty());
    assert_same_series(&f, a, b, &targets).await?;

    post_history(&f, &template, &h, 1).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;
    assert_same_series(&f, a, b, &targets).await?;
    Ok(())
}

#[tokio::test]
async fn backfilled_attachment_equals_preexisting() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let h = history_accounts(&f).await?;
    let a =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    attach_many_with_history(&f, a, &[h.r1], History::FromNow).await?;
    f.jobs.start_poll().await?;

    post_history(&f, &template, &h, 0).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;

    let b =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    f.cala.calculations().attach(b, h.r1).await?;
    wait_for_backfill(&f.cala, b, Some(h.r1)).await?;
    assert!(attachment_backfill_complete(&f, b, h.r1).await?);
    assert!(f
        .cala
        .calculations()
        .list_open_backfills(b)
        .await?
        .is_empty());
    assert_same_series(&f, a, b, &[h.r1]).await?;

    post_history(&f, &template, &h, 1).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;
    assert_same_series(&f, a, b, &[h.r1]).await?;
    Ok(())
}

#[tokio::test]
async fn handoff_waits_for_the_stalled_tip_and_lands_entries_exactly_once() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let a = create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    let h = history_accounts(&f).await?;
    let targets = h.targets(f.sender);
    f.jobs.start_poll().await?;
    post_history(&f, &template, &h, 0).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;

    let stall = hold_handoff_lock(&f.pool).await?;
    let c0 = checkpoint(&f).await?;
    post_many(&f, &template, h.r1, 10).await?;
    let b =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;

    wait_for_cursor(&f, b, c0).await?;
    assert_eq!(
        backfill_status(&f, b).await?,
        BackfillStatus::InProgress,
        "the chase reached the stalled checkpoint; the handoff waits for the tip"
    );
    assert_eq!(checkpoint(&f).await?, c0, "the tip is still stalled");
    post_many(&f, &template, h.r2, 5).await?;

    stall.rollback().await?;
    wait_for_backfill(&f.cala, b, None).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;
    let activation = activation_sequence(&f, b).await?;
    assert!(
        activation >= c0,
        "handoff at or above the stalled checkpoint"
    );
    assert!(activation <= checkpoint(&f).await?);
    assert_same_series(&f, a, b, &targets).await?;
    Ok(())
}

#[tokio::test]
async fn backlog_above_threshold_lands_exactly_once_after_release() -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let a = create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    let h = history_accounts(&f).await?;
    let targets = h.targets(f.sender);
    f.jobs.start_poll().await?;
    post_history(&f, &template, &h, 0).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;

    let stall = hold_handoff_lock(&f.pool).await?;
    let c0 = checkpoint(&f).await?;
    // 700 two-entry postings: 2,100 outbox events above the stalled
    // checkpoint, more than HANDOFF_THRESHOLD.
    post_many(&f, &template, h.r1, 700).await?;
    let b =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    wait_for_cursor(&f, b, c0).await?;
    assert_eq!(backfill_status(&f, b).await?, BackfillStatus::InProgress);

    stall.rollback().await?;
    wait_for_backfill(&f.cala, b, None).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;
    assert!(activation_sequence(&f, b).await? >= c0);
    assert_same_series(&f, a, b, &targets).await?;
    println!(
        "backlog_above_threshold_lands_exactly_once_after_release: {:?}",
        started.elapsed()
    );
    Ok(())
}

#[tokio::test]
async fn chunking() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let a = create_calculation_with_history(&f, CalculationScope::Global, History::FromNow).await?;
    let h = history_accounts(&f).await?;
    let targets = h.targets(f.sender);
    f.jobs.start_poll().await?;
    post_history(&f, &template, &h, 0).await?;
    // 550 two-entry postings: 1,100 entries, 1,650 collected events, two
    // chunks.
    post_many(&f, &template, h.r1, 550).await?;
    wait_for_ec(&f).await?;
    wait_for_calculations(&f.cala).await?;
    let c = checkpoint(&f).await?;

    let b =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    let mut observed = std::collections::BTreeSet::new();
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(cursor) = open_cursor(&f, b).await? {
            observed.insert(cursor);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        anyhow::Ok(())
    })
    .await??;
    wait_for_backfill(&f.cala, b, None).await?;
    assert!(
        observed.iter().any(|cursor| *cursor > 0 && *cursor < c),
        "an intermediate cursor was observed: {observed:?} (checkpoint {c})"
    );
    assert_same_series(&f, a, b, &targets).await?;
    Ok(())
}

#[tokio::test]
async fn attach_many_one_job() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let mut accounts = Vec::new();
    for _ in 0..50 {
        accounts.push(create_account(&f.cala).await?);
    }
    let a =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    attach_many_with_history(&f, a, &accounts, History::FromNow).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();
    let inputs: Vec<_> = accounts
        .iter()
        .enumerate()
        .map(|(i, account)| posting_input(&f, &template, f.sender, *account, i as i64 + 1, today))
        .collect();
    f.cala.post_transactions(inputs).await?;
    wait_for_calculations(&f.cala).await?;

    let b =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    let mut op = f.cala.begin_operation().await?;
    f.cala
        .calculations()
        .attach_many_in_op(&mut op, b, &accounts)
        .await?;
    op.commit().await?;
    let open = f.cala.calculations().list_open_backfills(b).await?;
    assert_eq!(open.len(), 1, "one backfill row for the call");
    assert_eq!(open[0].account_ids.as_ref().map(Vec::len), Some(50));
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE id = $1")
        .bind(uuid::Uuid::from(open[0].id))
        .fetch_one(&f.pool)
        .await?;
    assert_eq!(jobs, 1, "one job, keyed by the backfill id");

    wait_for_backfill(&f.cala, b, Some(accounts[0])).await?;
    for account in accounts.iter() {
        assert!(attachment_backfill_complete(&f, b, *account).await?);
    }
    assert_same_series(&f, a, b, &accounts).await?;
    Ok(())
}

#[tokio::test]
async fn fence_times_out_honestly() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    post_many(&f, &template, recipient, 20).await?;
    wait_for_calculations(&f.cala).await?;

    let stall = hold_handoff_lock(&f.pool).await?;
    let b =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    assert!(matches!(
        f.cala
            .calculations()
            .await_backfill(b, None, Duration::from_millis(500))
            .await,
        Err(Fail::Rejected(CalculationBackfillTimeout { .. }))
    ));

    stall.rollback().await?;
    wait_for_backfill(&f.cala, b, None).await?;
    Ok(())
}

#[tokio::test]
async fn outbox_event_on_completion() -> anyhow::Result<()> {
    let mut f = setup(helpers::test_journal_with_effective_balances()).await?;
    let template = create_template(&f.cala, "TEST").await?;
    let recipient = create_account(&f.cala).await?;
    f.jobs.start_poll().await?;
    post_many(&f, &template, recipient, 5).await?;
    wait_for_calculations(&f.cala).await?;

    let frontier = f.cala.calculation_rollup_status().await?.frontier;
    let mut listener = f.cala.register_outbox_listener(Some(frontier));
    let global =
        create_calculation_with_history(&f, CalculationScope::Global, History::Backfill).await?;
    let attached =
        create_calculation_with_history(&f, CalculationScope::Attached, History::FromNow).await?;
    f.cala.calculations().attach(attached, recipient).await?;
    wait_for_backfill(&f.cala, global, None).await?;
    wait_for_backfill(&f.cala, attached, Some(recipient)).await?;

    let stored_global = activation_sequence(&f, global).await?;
    let stored_attachment: i64 = sqlx::query_scalar(
        "SELECT activation_sequence FROM cala_calculation_attachments
         WHERE calculation_id = $1 AND account_id = $2",
    )
    .bind(attached)
    .bind(recipient)
    .fetch_one(&f.pool)
    .await?;

    let mut seen = std::collections::HashMap::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = listener.next().await {
            let event = event.expect("outbox delivery");
            if let Some(OutboxEventPayload::CalculationBackfillCompleted {
                calculation_id,
                account_ids,
                activation_sequence,
            }) = &event.payload
            {
                seen.insert(*calculation_id, (account_ids.clone(), *activation_sequence));
            }
            if seen.len() == 2 {
                break;
            }
        }
    })
    .await?;
    assert_eq!(seen.get(&global), Some(&(None, stored_global)));
    assert_eq!(
        seen.get(&attached),
        Some(&(Some(vec![recipient]), stored_attachment as u64))
    );
    Ok(())
}
