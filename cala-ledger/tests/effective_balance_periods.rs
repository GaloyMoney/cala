mod helpers;

use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use cala_ledger::{
    account_set::NewAccountSet,
    balance::{
        error::BalanceNotFound, AccountBalance, BalanceRange, EffectiveBalanceSeries,
        EffectivePeriods, InvalidEffectivePeriods, PeriodGranularity, MAX_EFFECTIVE_PERIODS_WORK,
    },
    errlanes::Fail,
    job::Jobs,
    primitives::BalanceRollup,
    tx_template::*,
    *,
};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn months(gte: NaiveDate, count: usize) -> EffectivePeriods {
    EffectivePeriods::try_from_count(PeriodGranularity::Month, gte, count).unwrap()
}

fn assert_balance_amounts_eq(actual: &AccountBalance, expected: &AccountBalance) {
    assert_eq!(actual.settled(), expected.settled());
    assert_eq!(actual.pending(), expected.pending());
    assert_eq!(actual.encumbrance(), expected.encumbrance());
}

fn assert_balance_amounts_sum(
    actual: &AccountBalance,
    first: &AccountBalance,
    second: &AccountBalance,
) {
    assert_eq!(actual.settled(), first.settled() + second.settled());
    assert_eq!(actual.pending(), first.pending() + second.pending());
    assert_eq!(
        actual.encumbrance(),
        first.encumbrance() + second.encumbrance()
    );
}

fn assert_balance_range_amounts_sum(
    actual: &BalanceRange,
    first: &BalanceRange,
    second: &BalanceRange,
) {
    assert_balance_amounts_sum(&actual.open, &first.open, &second.open);
    assert_balance_amounts_sum(&actual.period, &first.period, &second.period);
    assert_balance_amounts_sum(&actual.close, &first.close, &second.close);
}

fn assert_zero(balance: &AccountBalance) {
    assert_eq!(balance.settled(), Decimal::ZERO);
    assert_eq!(balance.pending(), Decimal::ZERO);
    assert_eq!(balance.encumbrance(), Decimal::ZERO);
}

fn assert_series_eq(actual: &EffectiveBalanceSeries, expected: &EffectiveBalanceSeries) {
    assert_eq!(actual.periods.len(), expected.periods.len());
    for (actual, expected) in actual.periods.iter().zip(&expected.periods) {
        assert_eq!((actual.start, actual.end), (expected.start, expected.end));
        assert_eq!(actual.range.open.details, expected.range.open.details);
        assert_eq!(actual.range.period.details, expected.range.period.details);
        assert_eq!(actual.range.close.details, expected.range.close.details);
    }
}

struct Fixture {
    cala: CalaLedger,
    jobs: Jobs,
    journal_id: JournalId,
    code: String,
    sender: AccountId,
    recipient: AccountId,
    /// Synchronous set containing `sender`.
    set: AccountId,
}

impl Fixture {
    async fn new(pool: sqlx::PgPool) -> anyhow::Result<Self> {
        let mut jobs = helpers::init_jobs(pool.clone()).await?;
        let cala_config = CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .build()?;
        let cala = CalaLedger::init(cala_config, &mut jobs).await?;
        let journal = cala
            .journals()
            .create(helpers::test_journal_with_effective_balances())
            .await?;
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        cala.tx_templates()
            .create(helpers::currency_conversion_template(&code))
            .await?;
        let (sender, recipient) = helpers::test_accounts();
        let sender = cala.accounts().create(sender).await?.id();
        let recipient = cala.accounts().create(recipient).await?.id();
        let set = cala
            .account_sets()
            .create(
                NewAccountSet::builder()
                    .id(AccountSetId::new())
                    .name("Periods Set")
                    .journal_id(journal.id())
                    .balance_rollup(BalanceRollup::Synchronous)
                    .build(),
            )
            .await?;
        cala.account_sets().add_member(set.id(), sender).await?;
        Ok(Self {
            cala,
            jobs,
            journal_id: journal.id(),
            code,
            sender,
            recipient,
            set: set.id().into(),
        })
    }

    async fn new_isolated() -> anyhow::Result<Self> {
        Self::new(helpers::init_isolated_pool().await?).await
    }

    async fn new_shared() -> anyhow::Result<Self> {
        Self::new(helpers::init_pool().await?).await
    }

    fn params(&self, sender: AccountId, recipient: AccountId, effective: NaiveDate) -> Params {
        let mut params = Params::new();
        params.insert("journal_id", self.journal_id.to_string());
        params.insert("sender", sender);
        params.insert("recipient", recipient);
        params.insert("effective", effective);
        params
    }

    async fn post(
        &self,
        sender: AccountId,
        recipient: AccountId,
        effective: NaiveDate,
    ) -> anyhow::Result<()> {
        self.cala
            .post_transaction(
                TransactionId::new(),
                &self.code,
                self.params(sender, recipient, effective),
            )
            .await?;
        Ok(())
    }

    async fn series(
        &self,
        account_id: AccountId,
        currency: Currency,
        periods: &EffectivePeriods,
    ) -> anyhow::Result<EffectiveBalanceSeries> {
        Ok(self
            .cala
            .balances()
            .effective()
            .find_periods(self.journal_id, account_id, currency, periods)
            .await?)
    }

    /// Each period's `close`, `period` (amounts and entry count) and `open`
    /// must equal what the single-date and single-range reads report.
    async fn assert_oracles(
        &self,
        account_id: AccountId,
        currency: Currency,
        periods: &EffectivePeriods,
    ) -> anyhow::Result<()> {
        let effective = self.cala.balances().effective();
        let series = self.series(account_id, currency, periods).await?;
        assert_eq!(series.periods.len(), periods.len());
        let mut previous_close: Option<&AccountBalance> = None;
        for (period, (start, end)) in series.periods.iter().zip(periods.bounds()) {
            assert_eq!((period.start, period.end), (start, end));
            let cumulative = effective
                .find_cumulative(self.journal_id, account_id, currency, end)
                .await?;
            assert_balance_amounts_eq(&period.range.close, &cumulative);
            let range = effective
                .find_in_range(self.journal_id, account_id, currency, start, Some(end))
                .await?;
            assert_balance_amounts_eq(&period.range.open, &range.open);
            assert_balance_amounts_eq(&period.range.period, &range.period);
            assert_eq!(
                period.range.period.details.version, range.period.details.version,
                "{start}..{end}",
            );
            if let Some(previous) = previous_close {
                assert_balance_amounts_eq(&period.range.open, previous);
            }
            previous_close = Some(&period.range.close);
        }
        Ok(())
    }
}

#[tokio::test]
async fn monthly_series_matches_range_and_cumulative_oracles() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 3, 20)).await?;
    f.post(f.sender, f.recipient, date(2024, 1, 25)).await?;

    let periods = months(date(2024, 1, 1), 3);
    for account in [f.sender, f.recipient, f.set] {
        for currency in [Currency::BTC, Currency::USD] {
            f.assert_oracles(account, currency, &periods).await?;
        }
    }

    let recipient = f.series(f.recipient, Currency::BTC, &periods).await?;
    let versions: Vec<_> = recipient
        .periods
        .iter()
        .map(|p| p.range.period.details.version)
        .collect();
    assert_eq!(versions, vec![2, 1, 1]);
    let total = recipient.total_activity().unwrap();
    assert_eq!(total.settled(), dec!(5160));
    assert_eq!(total.details.version, 4);
    assert_eq!(recipient.closing().unwrap().settled(), dec!(5160));
    assert_zero(recipient.opening().unwrap());
    Ok(())
}

#[tokio::test]
async fn daily_series_has_zero_days_and_exact_entry_counts() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 20)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 20)).await?;

    let periods =
        EffectivePeriods::try_from_count(PeriodGranularity::Day, date(2024, 2, 1), 29).unwrap();
    let series = f.series(f.recipient, Currency::BTC, &periods).await?;
    assert_eq!(series.periods.len(), 29);
    for (index, day) in series.periods.iter().enumerate() {
        let feb = index as u32 + 1;
        assert_eq!(day.start, date(2024, 2, feb));
        assert_eq!(day.end, day.start);
        let (activity, entries, close) = match feb {
            1..=4 => (dec!(0), 0, dec!(0)),
            5 => (dec!(1290), 1, dec!(1290)),
            6..=19 => (dec!(0), 0, dec!(1290)),
            20 => (dec!(2580), 2, dec!(3870)),
            _ => (dec!(0), 0, dec!(3870)),
        };
        assert_eq!(day.range.period.settled(), activity, "Feb {feb}");
        assert_eq!(day.range.period.details.version, entries, "Feb {feb}");
        assert_eq!(day.range.close.settled(), close, "Feb {feb}");
    }
    f.assert_oracles(f.recipient, Currency::BTC, &{
        EffectivePeriods::try_from_count(PeriodGranularity::Day, date(2024, 2, 5), 25).unwrap()
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn series_over_run_with_rows_only_before_gte() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 1, 20)).await?;

    let periods = months(date(2024, 2, 1), 3);
    let january = f
        .cala
        .balances()
        .effective()
        .find_cumulative(f.journal_id, f.recipient, Currency::BTC, date(2024, 1, 31))
        .await?;
    let series = f.series(f.recipient, Currency::BTC, &periods).await?;
    assert_eq!(series.periods.len(), 3);
    for period in &series.periods {
        assert_balance_amounts_eq(&period.range.open, &january);
        assert_balance_amounts_eq(&period.range.close, &january);
        assert_zero(&period.range.period);
        assert_eq!(period.range.period.details.version, 0);
    }
    assert_eq!(january.settled(), dec!(2580));
    Ok(())
}

#[tokio::test]
async fn absent_ids() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    let (late_sender, late_recipient) = helpers::test_accounts();
    let late_sender = f.cala.accounts().create(late_sender).await?.id();
    let late_recipient = f.cala.accounts().create(late_recipient).await?.id();
    f.post(late_sender, late_recipient, date(2024, 6, 10))
        .await?;

    let unknown = AccountId::new();
    let unused: Currency = "GBP".parse()?;
    let present = (f.journal_id, f.sender, Currency::BTC);
    let ids = [
        present,
        (f.journal_id, unknown, Currency::BTC),
        (f.journal_id, late_sender, Currency::BTC),
        (f.journal_id, f.sender, unused),
    ];
    let periods = months(date(2024, 1, 1), 3);
    let found = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&ids, &periods)
        .await?;
    assert_eq!(found.len(), 1);
    assert!(found.contains_key(&present));

    for (account, currency) in [
        (unknown, Currency::BTC),
        (late_sender, Currency::BTC),
        (f.sender, unused),
    ] {
        let result = f
            .cala
            .balances()
            .effective()
            .find_periods(f.journal_id, account, currency, &periods)
            .await;
        assert!(
            matches!(result, Err(Fail::Rejected(BalanceNotFound(..)))),
            "{account} {currency}",
        );
    }
    Ok(())
}

#[tokio::test]
async fn batch_equals_single() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 1, 25)).await?;

    let distinct = [
        (f.journal_id, f.sender, Currency::BTC),
        (f.journal_id, f.recipient, Currency::BTC),
        (f.journal_id, f.set, Currency::BTC),
        (f.journal_id, f.sender, Currency::USD),
    ];
    let mut ids = distinct.to_vec();
    ids.push(distinct[0]);
    ids.push(distinct[2]);

    let periods = months(date(2024, 1, 1), 4);
    let batch = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&ids, &periods)
        .await?;
    assert_eq!(batch.len(), distinct.len());
    for (journal_id, account_id, currency) in distinct {
        let single = f
            .cala
            .balances()
            .effective()
            .find_periods(journal_id, account_id, currency, &periods)
            .await?;
        assert_series_eq(&batch[&(journal_id, account_id, currency)], &single);
    }
    Ok(())
}

#[tokio::test]
async fn in_op_sees_uncommitted_entries() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    let effective_date = date(2025, 5, 5);
    let mut op = f.cala.begin_operation().await?;
    for _ in 0..2 {
        f.cala
            .post_transaction_in_op(
                &mut op,
                TransactionId::new(),
                &f.code,
                f.params(f.sender, f.recipient, effective_date),
            )
            .await?;
    }

    let periods = months(date(2025, 5, 1), 1);
    let id = (f.journal_id, f.recipient, Currency::BTC);
    let in_op = f
        .cala
        .balances()
        .effective()
        .find_all_periods_in_op(&mut op, &[id], &periods)
        .await?;
    let series = &in_op[&id];
    assert_eq!(series.periods.len(), 1);
    assert_eq!(series.periods[0].range.close.settled(), dec!(2580));
    assert_eq!(series.periods[0].range.period.details.version, 2);

    let outside = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&[id], &periods)
        .await?;
    assert!(outside.is_empty(), "not visible outside the open operation");

    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn ec_set_series_after_rollup() -> anyhow::Result<()> {
    let mut f = Fixture::new_isolated().await?;
    let (_, other) = helpers::test_accounts();
    let other = f.cala.accounts().create(other).await?.id();
    let ec_set = f
        .cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Periods EC Set")
                .journal_id(f.journal_id)
                .balance_rollup(BalanceRollup::EventuallyConsistent)
                .build(),
        )
        .await?;
    f.cala
        .account_sets()
        .add_member(ec_set.id(), f.recipient)
        .await?;
    f.cala.account_sets().add_member(ec_set.id(), other).await?;

    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, other, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 3, 20)).await?;
    f.post(f.sender, other, date(2024, 1, 25)).await?;

    let periods = months(date(2024, 1, 1), 3);
    let last_day = date(2024, 3, 31);
    let effective = f.cala.balances().effective();
    let first = effective
        .find_cumulative(f.journal_id, f.recipient, Currency::BTC, last_day)
        .await?;
    let second = effective
        .find_cumulative(f.journal_id, other, Currency::BTC, last_day)
        .await?;

    f.jobs.start_poll().await?;
    helpers::wait_for_effective(
        &f.cala,
        f.journal_id,
        ec_set.id(),
        Currency::BTC,
        last_day,
        first.settled() + second.settled(),
    )
    .await?;

    let set = f
        .series(AccountId::from(ec_set.id()), Currency::BTC, &periods)
        .await?;
    let first = f.series(f.recipient, Currency::BTC, &periods).await?;
    let second = f.series(other, Currency::BTC, &periods).await?;
    assert_eq!(set.periods.len(), 3);
    for ((set, first), second) in set.periods.iter().zip(&first.periods).zip(&second.periods) {
        assert_balance_range_amounts_sum(&set.range, &first.range, &second.range);
    }
    assert_eq!(set.closing().unwrap().settled(), dec!(5160));
    Ok(())
}

/// Counts `tracing` spans by exact name and every `sqlx::query` event (one
/// per statement the pool executes). Registered process-global; see
/// `velocity.rs`'s `SpanCallCounter` for why that is sound under nextest.
struct StatementCounter {
    span_name: &'static str,
    spans: Arc<AtomicUsize>,
    statements: Arc<AtomicUsize>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for StatementCounter {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == self.span_name {
            self.spans.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() == "sqlx::query" {
            self.statements.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn one_statement_per_call() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    let spans = Arc::new(AtomicUsize::new(0));
    let statements = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(
        StatementCounter {
            span_name: "cala_ledger.balances.effective.find_checkpoints",
            spans: spans.clone(),
            statements: statements.clone(),
        },
    ))
    .expect("no other global tracing subscriber should be set in this test process");

    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 7, 5)).await?;
    assert!(
        statements.load(Ordering::SeqCst) > 0,
        "the statement counter must observe the pool's queries",
    );

    let ids = [
        (f.journal_id, f.sender, Currency::BTC),
        (f.journal_id, f.recipient, Currency::BTC),
        (f.journal_id, f.set, Currency::BTC),
    ];
    let periods = months(date(2024, 1, 1), 12);
    spans.store(0, Ordering::SeqCst);
    statements.store(0, Ordering::SeqCst);
    let series = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&ids, &periods)
        .await?;

    assert_eq!(series.len(), 3);
    assert!(series.values().all(|s| s.periods.len() == 12));
    assert_eq!(spans.load(Ordering::SeqCst), 1);
    assert_eq!(statements.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn work_limit_rejects() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let f = Fixture::new(pool.clone()).await?;
    pool.close().await;

    let periods = months(date(2024, 1, 1), 12);
    let probes_per_id = periods.len() + 1;
    let at_limit = MAX_EFFECTIVE_PERIODS_WORK / probes_per_id;
    let ids_of = |count: usize| -> Vec<BalanceId> {
        (0..count)
            .map(|_| (f.journal_id, AccountId::new(), Currency::USD))
            .collect()
    };

    // With the pool closed any statement fails, so a rejection proves the
    // limit fired before one was issued.
    let rejected = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&ids_of(at_limit + 1), &periods)
        .await;
    assert!(matches!(
        rejected,
        Err(Fail::Rejected(InvalidEffectivePeriods))
    ));

    let within = f
        .cala
        .balances()
        .effective()
        .find_all_periods(&ids_of(at_limit), &periods)
        .await;
    assert!(within.is_err());
    assert!(!matches!(within, Err(Fail::Rejected(_))));
    Ok(())
}
