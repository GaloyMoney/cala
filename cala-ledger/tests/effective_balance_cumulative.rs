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
        error::BalanceNotFound, AccountBalance, BalanceRange, CumulativeBalances, EffectiveDates,
        EffectivePeriods, InvalidEffectiveDates, PeriodGranularity, MAX_EFFECTIVE_PERIODS_WORK,
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

fn dates(dates: impl IntoIterator<Item = NaiveDate>) -> EffectiveDates {
    EffectiveDates::try_new(dates).unwrap()
}

/// 26 dates, one week apart, from 2024-01-01.
fn weekly_dates() -> EffectiveDates {
    dates((0..26).map(|week| date(2024, 1, 1) + chrono::Days::new(7 * week)))
}

fn assert_amounts_eq(actual: &AccountBalance, expected: &AccountBalance, what: &str) {
    assert_eq!(actual.settled(), expected.settled(), "{what} settled");
    assert_eq!(actual.pending(), expected.pending(), "{what} pending");
    assert_eq!(
        actual.encumbrance(),
        expected.encumbrance(),
        "{what} encumbrance"
    );
    assert_eq!(actual.balance_type, expected.balance_type, "{what} type");
}

fn assert_range_eq(actual: &BalanceRange, expected: &BalanceRange, what: &str) {
    assert_amounts_eq(&actual.open, &expected.open, &format!("{what} open"));
    assert_amounts_eq(&actual.period, &expected.period, &format!("{what} period"));
    assert_amounts_eq(&actual.close, &expected.close, &format!("{what} close"));
    assert_eq!(
        actual.period.details.version, expected.period.details.version,
        "{what} version"
    );
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

fn assert_zero(balance: &AccountBalance) {
    assert_eq!(balance.settled(), Decimal::ZERO);
    assert_eq!(balance.pending(), Decimal::ZERO);
    assert_eq!(balance.encumbrance(), Decimal::ZERO);
    assert_eq!(balance.details.version, 0);
}

fn assert_cumulative_eq(actual: &CumulativeBalances, expected: &CumulativeBalances) {
    let actual: Vec<_> = actual.iter().collect();
    let expected: Vec<_> = expected.iter().collect();
    assert_eq!(actual.len(), expected.len());
    for ((actual_date, actual), (expected_date, expected)) in actual.iter().zip(&expected) {
        assert_eq!(actual_date, expected_date);
        assert_eq!(actual.details, expected.details, "{actual_date}");
        assert_eq!(actual.balance_type, expected.balance_type, "{actual_date}");
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
                    .name("Cumulative Set")
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

    async fn cumulative_at(
        &self,
        account_id: AccountId,
        currency: Currency,
        dates: &EffectiveDates,
    ) -> anyhow::Result<CumulativeBalances> {
        Ok(self
            .cala
            .balances()
            .effective()
            .find_cumulative_at(self.journal_id, account_id, currency, dates)
            .await?)
    }
}

#[tokio::test]
async fn unaligned_overlapping_ranges_match_find_all_in_range() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 3, 20)).await?;
    f.post(f.sender, f.recipient, date(2024, 1, 25)).await?;

    let requested = dates([
        date(2023, 12, 31),
        date(2024, 1, 17),
        date(2024, 2, 29),
        date(2024, 3, 17),
        date(2024, 3, 31),
    ]);
    let ranges = [
        (date(2024, 1, 1), date(2024, 3, 17)),
        (date(2024, 1, 18), date(2024, 2, 29)),
        (date(2024, 3, 1), date(2024, 3, 17)),
        (date(2024, 1, 1), date(2024, 3, 31)),
    ];
    let effective = f.cala.balances().effective();
    for account in [f.sender, f.recipient, f.set] {
        for currency in [Currency::BTC, Currency::USD] {
            let id = (f.journal_id, account, currency);
            let batch = effective.find_all_cumulative_at(&[id], &requested).await?;
            let cumulative = &batch[&id];
            assert_eq!(cumulative.iter().count(), requested.len());
            for (from, until) in ranges {
                let what = format!("{account} {currency} {from}..{until}");
                let range = cumulative.range(from, until).expect("requested bounds");
                let expected = effective
                    .find_all_in_range(&[id], from, Some(until))
                    .await?;
                assert_range_eq(&range, &expected[&id], &what);
            }
            for &d in requested.dates() {
                let what = format!("{account} {currency} at {d}");
                let at = cumulative.at(d).expect("requested date");
                match effective
                    .find_cumulative(f.journal_id, account, currency, d)
                    .await
                {
                    Ok(expected) => {
                        assert_amounts_eq(at, &expected, &what);
                        assert_eq!(at.details.version, expected.details.version, "{what}");
                    }
                    Err(Fail::Rejected(BalanceNotFound(..))) => assert_zero(at),
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }

    let recipient = f
        .cumulative_at(f.recipient, Currency::BTC, &requested)
        .await?;
    let overall = recipient
        .range(date(2024, 1, 1), date(2024, 3, 31))
        .unwrap();
    assert_eq!(overall.period.settled(), dec!(5160));
    assert_eq!(overall.period.details.version, 4);
    assert_zero(&overall.open);
    Ok(())
}

#[tokio::test]
async fn zero_before_first_row_and_carry_after_last() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;

    let requested = dates([date(2024, 1, 31), date(2024, 2, 5), date(2024, 6, 30)]);
    let cumulative = f
        .cumulative_at(f.recipient, Currency::BTC, &requested)
        .await?;

    let before = cumulative.at(date(2024, 1, 31)).unwrap();
    assert_zero(before);
    let on = cumulative.at(date(2024, 2, 5)).unwrap();
    assert_eq!(on.settled(), dec!(1290));
    let after = cumulative.at(date(2024, 6, 30)).unwrap();
    assert_eq!(after.details, on.details);

    let first = cumulative
        .range(date(2024, 2, 1), date(2024, 2, 5))
        .unwrap();
    assert_zero(&first.open);
    assert_eq!(first.period.settled(), dec!(1290));
    assert_eq!(first.period.details.version, 1);

    let carried = cumulative
        .range(date(2024, 2, 6), date(2024, 6, 30))
        .unwrap();
    assert_eq!(carried.open.details, on.details);
    assert_eq!(carried.period.settled(), Decimal::ZERO);
    assert_eq!(carried.period.details.version, 0);
    assert_eq!(carried.close.details, on.details);
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
    let requested = dates([date(2023, 12, 31), date(2024, 1, 17), date(2024, 3, 31)]);
    let found = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&ids, &requested)
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
            .find_cumulative_at(f.journal_id, account, currency, &requested)
            .await;
        assert!(
            matches!(result, Err(Fail::Rejected(BalanceNotFound(..)))),
            "{account} {currency}",
        );
    }
    Ok(())
}

#[tokio::test]
async fn batch_equals_single_and_dedups_ids() -> anyhow::Result<()> {
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

    let requested = dates([
        date(2024, 1, 31),
        date(2023, 12, 31),
        date(2024, 1, 17),
        date(2024, 1, 31),
        date(2024, 4, 15),
    ]);
    assert_eq!(requested.len(), 4);
    let batch = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&ids, &requested)
        .await?;
    assert_eq!(batch.len(), distinct.len());
    for (journal_id, account_id, currency) in distinct {
        let single = f.cumulative_at(account_id, currency, &requested).await?;
        assert_cumulative_eq(&batch[&(journal_id, account_id, currency)], &single);
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

    let requested = dates([date(2025, 4, 30), date(2025, 5, 17)]);
    let id = (f.journal_id, f.recipient, Currency::BTC);
    let in_op = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at_in_op(&mut op, &[id], &requested)
        .await?;
    let range = in_op[&id]
        .range(date(2025, 5, 1), date(2025, 5, 17))
        .unwrap();
    assert_eq!(range.close.settled(), dec!(2580));
    assert_eq!(range.period.details.version, 2);

    let outside = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&[id], &requested)
        .await?;
    assert!(outside.is_empty(), "not visible outside the open operation");

    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn ec_set_after_rollup() -> anyhow::Result<()> {
    let mut f = Fixture::new_isolated().await?;
    let (_, other) = helpers::test_accounts();
    let other = f.cala.accounts().create(other).await?.id();
    let ec_set = f
        .cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Cumulative EC Set")
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

    let requested = dates([date(2024, 1, 17), date(2024, 3, 24), last_day]);
    let set_id = (f.journal_id, AccountId::from(ec_set.id()), Currency::BTC);
    let first_id = (f.journal_id, f.recipient, Currency::BTC);
    let second_id = (f.journal_id, other, Currency::BTC);
    let all = effective
        .find_all_cumulative_at(&[set_id, first_id, second_id], &requested)
        .await?;
    let (from, until) = (date(2024, 1, 18), date(2024, 3, 24));
    let set = all[&set_id].range(from, until).unwrap();
    let first = all[&first_id].range(from, until).unwrap();
    let second = all[&second_id].range(from, until).unwrap();
    assert_balance_amounts_sum(&set.open, &first.open, &second.open);
    assert_balance_amounts_sum(&set.period, &first.period, &second.period);
    assert_balance_amounts_sum(&set.close, &first.close, &second.close);
    assert_eq!(set.period.settled(), dec!(3870));
    assert_eq!(all[&set_id].at(last_day).unwrap().settled(), dec!(5160));
    Ok(())
}

/// Counts `tracing` spans by exact name and the `sqlx::query` events (one per
/// executed statement) of statements that touch the cumulative effective
/// balance table; unrelated background statements are ignored. Registered
/// process-global; see
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
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut statement = StatementText(String::new());
        event.record(&mut statement);
        if statement.0.contains("cala_cumulative_effective_balances") {
            self.statements.fetch_add(1, Ordering::SeqCst);
        }
    }
}

struct StatementText(String);

impl tracing::field::Visit for StatementText {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" {
            self.0 = format!("{value:?}");
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
    f.post(f.sender, f.recipient, date(2024, 4, 5)).await?;
    assert!(
        statements.load(Ordering::SeqCst) > 0,
        "the statement counter must observe the pool's queries",
    );

    let ids = [
        (f.journal_id, f.sender, Currency::BTC),
        (f.journal_id, f.recipient, Currency::BTC),
        (f.journal_id, f.set, Currency::BTC),
    ];
    let requested = weekly_dates();
    assert_eq!(requested.len(), 26);
    spans.store(0, Ordering::SeqCst);
    statements.store(0, Ordering::SeqCst);
    let cumulative = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&ids, &requested)
        .await?;

    assert_eq!(cumulative.len(), 3);
    assert!(cumulative.values().all(|c| c.iter().count() == 26));
    assert_eq!(spans.load(Ordering::SeqCst), 1);
    assert_eq!(statements.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn work_limit_rejects_before_statement() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let f = Fixture::new(pool.clone()).await?;
    // `close()` marks the pool closed as soon as it is called. Awaiting it
    // would wait for every checked-out connection (sqlx 0.9), and the job
    // service's listener holds one for the fixture's lifetime.
    drop(pool.close());
    assert!(pool.is_closed());

    let requested = weekly_dates();
    let at_limit = MAX_EFFECTIVE_PERIODS_WORK / requested.len();
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
        .find_all_cumulative_at(&ids_of(at_limit + 1), &requested)
        .await;
    assert!(matches!(
        rejected,
        Err(Fail::Rejected(InvalidEffectiveDates))
    ));

    let within = f
        .cala
        .balances()
        .effective()
        .find_all_cumulative_at(&ids_of(at_limit), &requested)
        .await;
    assert!(within.is_err());
    assert!(!matches!(within, Err(Fail::Rejected(_))));
    Ok(())
}

#[tokio::test]
async fn periods_equals_cumulative_projection() -> anyhow::Result<()> {
    let f = Fixture::new_shared().await?;
    f.post(f.sender, f.recipient, date(2024, 1, 10)).await?;
    f.post(f.sender, f.recipient, date(2024, 2, 5)).await?;
    f.post(f.sender, f.recipient, date(2024, 3, 20)).await?;
    f.post(f.sender, f.recipient, date(2024, 1, 25)).await?;

    let periods =
        EffectivePeriods::try_from_count(PeriodGranularity::Month, date(2023, 12, 1), 6).unwrap();
    let gte = periods.bounds().next().expect("non-empty").0;
    let requested =
        dates(std::iter::once(gte.pred_opt().unwrap()).chain(periods.bounds().map(|(_, end)| end)));
    assert_eq!(requested.len(), periods.len() + 1);

    let ids = [
        (f.journal_id, f.sender, Currency::BTC),
        (f.journal_id, f.recipient, Currency::BTC),
        (f.journal_id, f.set, Currency::BTC),
        (f.journal_id, f.recipient, Currency::USD),
    ];
    let effective = f.cala.balances().effective();
    let series = effective.find_all_periods(&ids, &periods).await?;
    let cumulative = effective.find_all_cumulative_at(&ids, &requested).await?;
    assert_eq!(series.len(), ids.len());
    assert_eq!(cumulative.len(), ids.len());
    for id in ids {
        let series = &series[&id];
        let cumulative = &cumulative[&id];
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
