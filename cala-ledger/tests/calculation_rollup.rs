//! The calculation rollup is a global outbox consumer, so every test here
//! runs against its own isolated database.
mod helpers;

use chrono::NaiveDate;
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
        AccountBalance, EffectivePeriods, InvalidEffectivePeriods, PeriodGranularity,
        MAX_EFFECTIVE_PERIODS_WORK,
    },
    calculation::{error::CalculationBalanceNotFound, *},
    errlanes::Fail,
    job::Jobs,
    journal::NewJournal,
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

async fn wait_for_calculations(cala: &CalaLedger) -> anyhow::Result<()> {
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
    errors: Arc<AtomicUsize>,
}

impl Counter {
    fn new(span: &'static str) -> Self {
        Self {
            span,
            flushes: Arc::default(),
            statements: Arc::default(),
            errors: Arc::default(),
        }
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
    let attached = create_calculation(&f, CalculationScope::Attached, Config::default()).await?;
    f.jobs.start_poll().await?;
    let today = f.cala.clock().now().date_naive();

    post(&f, Post::new(&template, f.sender, r1, 1, today)).await?;
    let global = create_calculation(&f, CalculationScope::Global, Config::default()).await?;
    post(&f, Post::new(&template, f.sender, r1, 10, today)).await?;
    wait_for_calculations(&f.cala).await?;
    let rows = calculation_rows(&f, global, r1, json!({})).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, Decimal::from(10), "only B's credit");
    assert_eq!(rows[0].9, 1, "all_time_version counts B's entry only");

    post(&f, Post::new(&template, f.sender, r2, 100, today)).await?;
    f.cala.calculations().attach(attached, r2).await?;
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
async fn no_calculations_costs_one_statement() -> anyhow::Result<()> {
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
        statements, flushes,
        "one EXISTS probe per flush without calculations"
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
