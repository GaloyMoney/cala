mod helpers;

use chrono::{NaiveDate, Utc};
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;
use std::collections::{HashMap, HashSet};

use cala_ledger::{
    account_set::NewAccountSet,
    balance::{AccountBalance, BalanceRange, EffectiveBalanceSnapshot},
    tx_template::*,
    *,
};

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

fn assert_balance_range_details_eq(actual: &BalanceRange, expected: &BalanceRange) {
    assert_eq!(actual.open.balance_type, expected.open.balance_type);
    assert_eq!(actual.open.details, expected.open.details);
    assert_eq!(actual.period.balance_type, expected.period.balance_type);
    assert_eq!(actual.period.details, expected.period.details);
    assert_eq!(actual.close.balance_type, expected.close.balance_type);
    assert_eq!(actual.close.details, expected.close.details);
}

fn assert_balance_range_amounts_eq(actual: &BalanceRange, expected: &BalanceRange) {
    assert_balance_amounts_eq(&actual.open, &expected.open);
    assert_balance_amounts_eq(&actual.period, &expected.period);
    assert_balance_amounts_eq(&actual.close, &expected.close);
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

fn all_balances_query<C: std::fmt::Debug>() -> es_entity::PaginatedQueryArgs<C> {
    es_entity::PaginatedQueryArgs {
        first: 100,
        after: None,
    }
}

fn balances_by_currency<C>(
    balances: es_entity::PaginatedQueryRet<AccountBalance, C>,
) -> HashMap<Currency, AccountBalance> {
    balances
        .entities
        .into_iter()
        .map(|balance| (balance.details.currency, balance))
        .collect()
}

fn balances_by_id<C>(
    balances: es_entity::PaginatedQueryRet<AccountBalance, C>,
) -> HashMap<BalanceId, AccountBalance> {
    balances
        .entities
        .into_iter()
        .map(|balance| {
            (
                (
                    balance.details.journal_id,
                    balance.details.account_id,
                    balance.details.currency,
                ),
                balance,
            )
        })
        .collect()
}

fn ranges_by_currency<C>(
    ranges: es_entity::PaginatedQueryRet<BalanceRange, C>,
) -> HashMap<Currency, BalanceRange> {
    ranges
        .entities
        .into_iter()
        .map(|range| (range.close.details.currency, range))
        .collect()
}

fn ranges_by_id<C>(
    ranges: es_entity::PaginatedQueryRet<BalanceRange, C>,
) -> HashMap<BalanceId, BalanceRange> {
    ranges
        .entities
        .into_iter()
        .map(|range| {
            (
                (
                    range.close.details.journal_id,
                    range.close.details.account_id,
                    range.close.details.currency,
                ),
                range,
            )
        })
        .collect()
}

#[tokio::test]
async fn transaction_post_with_effective_balances() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal_with_effective_balances();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::currency_conversion_template(&tx_code);

    cala.tx_templates().create(new_template).await.unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    let date1 = NaiveDate::from_ymd_opt(2025, 5, 5).unwrap();
    params.insert("effective", date1);

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(1290));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(100));
    assert_eq!(recipient_balance.pending(), dec!(100));

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    let date2 = NaiveDate::from_ymd_opt(2025, 5, 4).unwrap();
    params.insert("effective", date2);

    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(1290));
    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date2)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(100));
    assert_eq!(recipient_balance.pending(), dec!(100));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(2580));

    let recipient_balance = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date1)
        .await?;
    assert_eq!(recipient_balance.settled(), dec!(200));
    assert_eq!(recipient_balance.pending(), dec!(200));

    let balances = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            Currency::USD,
            date1,
            Some(date1),
        )
        .await?;
    assert_eq!(balances.period.details.version, 2);
    assert_eq!(balances.period.settled(), dec!(100));
    assert_eq!(balances.period.pending(), dec!(100));

    let balances = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            Currency::USD,
            date2,
            None,
        )
        .await?;
    assert_eq!(balances.period.details.version, 4);
    assert_eq!(balances.period.settled(), dec!(200));
    assert_eq!(balances.period.pending(), dec!(200));

    Ok(())
}

#[tokio::test]
async fn list_cumulative_balances_for_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let date = NaiveDate::from_ymd_opt(2025, 6, 10).unwrap();
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let balances = cala
        .balances()
        .effective()
        .list_cumulative_for_account(
            journal.id(),
            recipient_account.id(),
            date,
            all_balances_query(),
        )
        .await?;
    let balances = balances_by_currency(balances);
    let currencies: HashSet<_> = balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let btc = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date)
        .await?;
    assert_eq!(balances[&Currency::BTC].balance_type, btc.balance_type);
    assert_eq!(balances[&Currency::BTC].details, btc.details);

    let usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::USD, date)
        .await?;
    assert_eq!(balances[&Currency::USD].balance_type, usd.balance_type);
    assert_eq!(balances[&Currency::USD].details, usd.details);

    let fresh = cala.accounts().create(helpers::test_accounts().0).await?;
    let empty = cala
        .balances()
        .effective()
        .list_cumulative_for_account(journal.id(), fresh.id(), date, all_balances_query())
        .await?;
    let empty = balances_by_currency(empty);
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn list_cumulative_balances_for_accounts() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let date = NaiveDate::from_ymd_opt(2025, 6, 11).unwrap();
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let expected_ids = [
        (journal.id(), recipient_account.id(), Currency::BTC),
        (journal.id(), recipient_account.id(), Currency::USD),
        (journal.id(), sender_account.id(), Currency::BTC),
        (journal.id(), sender_account.id(), Currency::USD),
    ];
    let expected = cala
        .balances()
        .effective()
        .find_all_cumulative(&expected_ids, date)
        .await?;

    let actual = cala
        .balances()
        .effective()
        .list_cumulative_for_accounts(
            journal.id(),
            &[recipient_account.id(), sender_account.id()],
            date,
            all_balances_query(),
        )
        .await?;
    let actual = balances_by_id(actual);

    assert_eq!(
        actual.keys().copied().collect::<HashSet<_>>(),
        expected.keys().copied().collect::<HashSet<_>>()
    );
    for id in expected.keys() {
        assert_eq!(actual[id].balance_type, expected[id].balance_type);
        assert_eq!(actual[id].details, expected[id].details);
    }

    Ok(())
}

#[tokio::test]
async fn list_cumulative_balances_for_account_sets() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_one = cala.accounts().create(receiver).await?;
    let (_, receiver_two) = helpers::test_accounts();
    let recipient_two = cala.accounts().create(receiver_two).await?;

    let inline_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Inline Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;
    let ec_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("EC Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::EventuallyConsistent)
                .build()?,
        )
        .await?;
    cala.account_sets()
        .add_member(inline_set.id(), recipient_one.id())
        .await?;
    cala.account_sets()
        .add_member(inline_set.id(), recipient_two.id())
        .await?;
    cala.account_sets()
        .add_member(ec_set.id(), recipient_one.id())
        .await?;
    cala.account_sets()
        .add_member(ec_set.id(), recipient_two.id())
        .await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let date = NaiveDate::from_ymd_opt(2025, 6, 12).unwrap();
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_one.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_two.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let inline_balances = cala
        .balances()
        .effective()
        .list_cumulative_for_account(
            journal.id(),
            AccountId::from(inline_set.id()),
            date,
            all_balances_query(),
        )
        .await?;
    let inline_balances = balances_by_currency(inline_balances);
    let currencies: HashSet<_> = inline_balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let inline_btc = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date)
        .await?;
    assert_eq!(
        inline_balances[&Currency::BTC].balance_type,
        inline_btc.balance_type
    );
    assert_eq!(inline_balances[&Currency::BTC].details, inline_btc.details);

    let inline_usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::USD, date)
        .await?;
    assert_eq!(
        inline_balances[&Currency::USD].balance_type,
        inline_usd.balance_type
    );
    assert_eq!(inline_balances[&Currency::USD].details, inline_usd.details);

    let recipient_one_btc = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_one.id(), Currency::BTC, date)
        .await?;
    let recipient_two_btc = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_two.id(), Currency::BTC, date)
        .await?;
    assert_balance_amounts_sum(
        &inline_balances[&Currency::BTC],
        &recipient_one_btc,
        &recipient_two_btc,
    );

    let recipient_one_usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_one.id(), Currency::USD, date)
        .await?;
    let recipient_two_usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_two.id(), Currency::USD, date)
        .await?;
    assert_balance_amounts_sum(
        &inline_balances[&Currency::USD],
        &recipient_one_usd,
        &recipient_two_usd,
    );

    let ec_before_rollup = cala
        .balances()
        .effective()
        .list_cumulative_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            date,
            all_balances_query(),
        )
        .await?;
    let ec_before_rollup = balances_by_currency(ec_before_rollup);
    assert!(ec_before_rollup.is_empty());

    // The streaming rollup populates the EC set asynchronously.
    jobs.start_poll().await?;
    for currency in [Currency::BTC, Currency::USD] {
        helpers::wait_for_effective(
            &cala,
            journal.id(),
            ec_set.id(),
            currency,
            date,
            inline_balances[&currency].settled(),
        )
        .await?;
    }

    let ec_balances = cala
        .balances()
        .effective()
        .list_cumulative_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            date,
            all_balances_query(),
        )
        .await?;
    let ec_balances = balances_by_currency(ec_balances);
    let currencies: HashSet<_> = ec_balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    for currency in [Currency::BTC, Currency::USD] {
        assert_balance_amounts_eq(&ec_balances[&currency], &inline_balances[&currency]);
    }

    Ok(())
}

#[tokio::test]
async fn list_range_balances_for_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let from = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
    let before_from = NaiveDate::from_ymd_opt(2025, 6, 19).unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", before_from);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", from);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let ranges = cala
        .balances()
        .effective()
        .list_in_range_for_account(
            journal.id(),
            recipient_account.id(),
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let ranges = ranges_by_currency(ranges);
    let currencies: HashSet<_> = ranges.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let btc = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            Currency::BTC,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_details_eq(&ranges[&Currency::BTC], &btc);

    let usd = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_account.id(),
            Currency::USD,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_details_eq(&ranges[&Currency::USD], &usd);

    let fresh = cala.accounts().create(helpers::test_accounts().0).await?;
    let empty = cala
        .balances()
        .effective()
        .list_in_range_for_account(
            journal.id(),
            fresh.id(),
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let empty = ranges_by_currency(empty);
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn list_range_balances_for_accounts() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let from = NaiveDate::from_ymd_opt(2025, 6, 21).unwrap();
    let before_from = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", before_from);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", from);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let expected_ids = [
        (journal.id(), recipient_account.id(), Currency::BTC),
        (journal.id(), recipient_account.id(), Currency::USD),
        (journal.id(), sender_account.id(), Currency::BTC),
        (journal.id(), sender_account.id(), Currency::USD),
    ];
    let expected = cala
        .balances()
        .effective()
        .find_all_in_range(&expected_ids, from, Some(from))
        .await?;

    let actual = cala
        .balances()
        .effective()
        .list_in_range_for_accounts(
            journal.id(),
            &[recipient_account.id(), sender_account.id()],
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let actual = ranges_by_id(actual);

    assert_eq!(
        actual.keys().copied().collect::<HashSet<_>>(),
        expected.keys().copied().collect::<HashSet<_>>()
    );
    for id in expected.keys() {
        assert_balance_range_details_eq(&actual[id], &expected[id]);
    }

    Ok(())
}

#[tokio::test]
async fn list_range_balances_for_account_sets() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_one = cala.accounts().create(receiver).await?;
    let (_, receiver_two) = helpers::test_accounts();
    let recipient_two = cala.accounts().create(receiver_two).await?;

    let inline_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Inline Range Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;
    let ec_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("EC Range Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::EventuallyConsistent)
                .build()?,
        )
        .await?;
    cala.account_sets()
        .add_member(inline_set.id(), recipient_one.id())
        .await?;
    cala.account_sets()
        .add_member(inline_set.id(), recipient_two.id())
        .await?;
    cala.account_sets()
        .add_member(ec_set.id(), recipient_one.id())
        .await?;
    cala.account_sets()
        .add_member(ec_set.id(), recipient_two.id())
        .await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let from = NaiveDate::from_ymd_opt(2025, 6, 22).unwrap();
    let before_from = NaiveDate::from_ymd_opt(2025, 6, 21).unwrap();

    for (recipient, effective) in [
        (recipient_one.id(), before_from),
        (recipient_two.id(), before_from),
        (recipient_one.id(), from),
        (recipient_two.id(), from),
    ] {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient);
        params.insert("effective", effective);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await?;
    }

    let inline_ranges = cala
        .balances()
        .effective()
        .list_in_range_for_account(
            journal.id(),
            AccountId::from(inline_set.id()),
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let inline_ranges = ranges_by_currency(inline_ranges);
    let currencies: HashSet<_> = inline_ranges.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let inline_btc = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            inline_set.id().into(),
            Currency::BTC,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_details_eq(&inline_ranges[&Currency::BTC], &inline_btc);

    let inline_usd = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            inline_set.id().into(),
            Currency::USD,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_details_eq(&inline_ranges[&Currency::USD], &inline_usd);

    let recipient_one_btc = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_one.id(),
            Currency::BTC,
            from,
            Some(from),
        )
        .await?;
    let recipient_two_btc = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_two.id(),
            Currency::BTC,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_amounts_sum(
        &inline_ranges[&Currency::BTC],
        &recipient_one_btc,
        &recipient_two_btc,
    );

    let recipient_one_usd = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_one.id(),
            Currency::USD,
            from,
            Some(from),
        )
        .await?;
    let recipient_two_usd = cala
        .balances()
        .effective()
        .find_in_range(
            journal.id(),
            recipient_two.id(),
            Currency::USD,
            from,
            Some(from),
        )
        .await?;
    assert_balance_range_amounts_sum(
        &inline_ranges[&Currency::USD],
        &recipient_one_usd,
        &recipient_two_usd,
    );

    let ec_before_rollup = cala
        .balances()
        .effective()
        .list_in_range_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let ec_before_rollup = ranges_by_currency(ec_before_rollup);
    assert!(ec_before_rollup.is_empty());

    // The streaming rollup populates the EC set asynchronously; wait for
    // cumulative convergence (range balances derive from the same snapshots).
    jobs.start_poll().await?;
    for currency in [Currency::BTC, Currency::USD] {
        let inline_cumulative = cala
            .balances()
            .effective()
            .find_cumulative(journal.id(), inline_set.id(), currency, from)
            .await?;
        helpers::wait_for_effective(
            &cala,
            journal.id(),
            ec_set.id(),
            currency,
            from,
            inline_cumulative.settled(),
        )
        .await?;
    }

    let ec_ranges = cala
        .balances()
        .effective()
        .list_in_range_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            from,
            Some(from),
            all_balances_query(),
        )
        .await?;
    let ec_ranges = ranges_by_currency(ec_ranges);
    let currencies: HashSet<_> = ec_ranges.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    for currency in [Currency::BTC, Currency::USD] {
        assert_balance_range_amounts_eq(&ec_ranges[&currency], &inline_ranges[&currency]);
    }

    Ok(())
}

/// The streaming rollup must maintain an EC set's *cumulative effective*
/// balances, including for **back-dated** transactions (which drive the
/// effective path's delete-future / replay logic). Verified against an
/// inline (non-EC) set maintained synchronously by the poster path.
#[tokio::test]
async fn ec_account_set_effective_balance_streaming() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala
        .journals()
        .create(helpers::test_journal_with_effective_balances())
        .await
        .unwrap();

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await
        .unwrap();

    // Inline set — effective balances updated inline on post.
    let inline_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Inline Set")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::Synchronous)
        .build()
        .unwrap();
    let inline_set = cala.account_sets().create(inline_set).await.unwrap();

    // EC set — effective balances maintained by the streaming rollup.
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC Set")
        .journal_id(journal.id())
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()
        .unwrap();
    let ec_set = cala.account_sets().create(ec_set).await.unwrap();

    cala.account_sets()
        .add_member(inline_set.id(), recipient_account.id())
        .await
        .unwrap();
    cala.account_sets()
        .add_member(ec_set.id(), recipient_account.id())
        .await
        .unwrap();

    // Post 3 transactions; the last is back-dated *between* the first two.
    let date1 = NaiveDate::from_ymd_opt(2025, 3, 10).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2025, 3, 20).unwrap();
    let date3 = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
    for date in [date1, date2, date3] {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params.insert("effective", date);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await
            .unwrap();
    }

    jobs.start_poll().await?;

    // Inline effective balances are maintained synchronously.
    let inline_d2 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(inline_d2.settled(), dec!(3870));

    // Wait for the rollup to fold all three transactions (incl. the
    // back-dated one) into the EC set's cumulative effective balance.
    helpers::wait_for_effective(
        &cala,
        journal.id(),
        ec_set.id(),
        Currency::BTC,
        date2,
        inline_d2.settled(),
    )
    .await?;

    // EC cumulative effective balances now match inline at every date.
    for (date, expected) in [
        (date1, dec!(1290)),
        (date3, dec!(2580)),
        (date2, dec!(3870)),
    ] {
        let ec = cala
            .balances()
            .effective()
            .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date)
            .await?;
        let inline = cala
            .balances()
            .effective()
            .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date)
            .await?;
        assert_eq!(
            ec.settled(),
            inline.settled(),
            "BTC (EC vs inline) at {date}"
        );
        assert_eq!(ec.settled(), expected, "BTC expected at {date}");
    }

    // USD settled + pending at date2 match inline too.
    let inline_usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::USD, date2)
        .await?;
    let ec_usd = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::USD, date2)
        .await?;
    assert_eq!(
        ec_usd.settled(),
        inline_usd.settled(),
        "USD settled at date2"
    );
    assert_eq!(
        ec_usd.pending(),
        inline_usd.pending(),
        "USD pending at date2"
    );

    Ok(())
}

/// Basic day-boundary case: a watermark captured between two postings on
/// different days must return only the later day's tuples.
#[tokio::test]
async fn list_modified_since_basic() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let day_before = NaiveDate::from_ymd_opt(2025, 7, 1).unwrap();
    let day_after = NaiveDate::from_ymd_opt(2025, 7, 2).unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", day_before);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let since = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", day_after);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let page = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), since, all_balances_query())
        .await?;
    assert!(!page.has_next_page);

    let tuples: HashSet<(AccountId, Currency, NaiveDate)> = page
        .entities
        .iter()
        .map(|s| (s.account_id, s.currency, s.effective))
        .collect();
    let expected: HashSet<(AccountId, Currency, NaiveDate)> = [
        (recipient_account.id(), Currency::BTC, day_after),
        (recipient_account.id(), Currency::USD, day_after),
        (sender_account.id(), Currency::BTC, day_after),
        (sender_account.id(), Currency::USD, day_after),
    ]
    .into_iter()
    .collect();
    assert_eq!(tuples, expected, "only the later day's tuples must appear");

    for snapshot in &page.entities {
        let fresh = cala
            .balances()
            .effective()
            .find_cumulative(
                journal.id(),
                snapshot.account_id,
                snapshot.currency,
                day_after,
            )
            .await?;
        assert_eq!(fresh.details.settled, snapshot.settled);
        assert_eq!(fresh.details.pending, snapshot.pending);
        assert_eq!(fresh.details.encumbrance, snapshot.encumbrance);
    }

    Ok(())
}

/// Backdating fan-out: seeding four consecutive days then posting a
/// backdated entry on the earliest of them rewrites every later snapshot
/// (fresh `modified_at`, corrected values) — all four days must come back.
#[tokio::test]
async fn list_modified_since_backdating_fanout() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let dates: Vec<NaiveDate> = (1..=4)
        .map(|d| NaiveDate::from_ymd_opt(2025, 8, d).unwrap())
        .collect();

    for &date in &dates {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params.insert("effective", date);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await?;
    }

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let since = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Backdated posting at the earliest date rewrites every later snapshot
    // (find_for_update deletes+replays every row with effective > this date).
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", dates[0]);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let page = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), since, all_balances_query())
        .await?;

    let recipient_btc: HashMap<NaiveDate, EffectiveBalanceSnapshot> = page
        .entities
        .into_iter()
        .filter(|s| s.account_id == recipient_account.id() && s.currency == Currency::BTC)
        .map(|s| (s.effective, s))
        .collect();

    assert_eq!(
        recipient_btc.keys().copied().collect::<HashSet<_>>(),
        dates.iter().copied().collect::<HashSet<_>>(),
        "backdating must rewrite every later snapshot, not just the backdated date"
    );

    for &date in &dates {
        let fresh = cala
            .balances()
            .effective()
            .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date)
            .await?;
        let snapshot = &recipient_btc[&date];
        assert_eq!(fresh.details.settled, snapshot.settled, "settled at {date}");
        assert_eq!(fresh.details.pending, snapshot.pending, "pending at {date}");
    }

    Ok(())
}

/// Several updates to the same tuple since the watermark must collapse to
/// exactly one row: the tuple's overall-latest snapshot.
#[tokio::test]
async fn list_modified_since_latest_wins() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let date = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let since = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    for _ in 0..3 {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params.insert("effective", date);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await?;
    }

    let page = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), since, all_balances_query())
        .await?;

    let recipient_btc_rows: Vec<&EffectiveBalanceSnapshot> = page
        .entities
        .iter()
        .filter(|s| s.account_id == recipient_account.id() && s.currency == Currency::BTC)
        .collect();
    assert_eq!(
        recipient_btc_rows.len(),
        1,
        "three updates to one tuple must collapse to a single latest row"
    );
    assert_eq!(
        recipient_btc_rows[0].version, 3,
        "should carry the third (latest) version at this date"
    );

    let fresh = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), recipient_account.id(), Currency::BTC, date)
        .await?;
    assert_eq!(fresh.details.settled, recipient_btc_rows[0].settled);

    Ok(())
}

/// Keyset pagination: stable, complete, non-overlapping pages across a
/// changed-set larger than one page, plus edge cases — a single-row page,
/// resuming exactly at a cursor boundary, and an empty result once the
/// watermark moves past all activity.
#[tokio::test]
async fn list_modified_since_pagination() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, _) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let since = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Five distinct recipients posted-to once, on the same date — a sparse
    // set of tuples (recipient UUIDs interleave with the sender across two
    // currencies), not a contiguous block.
    let date = NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();
    let mut recipients = Vec::new();
    for _ in 0..5 {
        let (_, receiver) = helpers::test_accounts();
        let recipient_account = cala.accounts().create(receiver).await?;
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender_account.id());
        params.insert("recipient", recipient_account.id());
        params.insert("effective", date);
        cala.post_transaction(TransactionId::new(), &tx_code, params)
            .await?;
        recipients.push(recipient_account.id());
    }
    let expected_count = recipients.len() * 2 + 2; // each recipient x{BTC,USD} + sender x{BTC,USD}

    let full = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), since, all_balances_query())
        .await?;
    assert!(!full.has_next_page);
    assert_eq!(full.entities.len(), expected_count);
    let expected_tuples: HashSet<(AccountId, Currency, NaiveDate)> = full
        .entities
        .iter()
        .map(|s| (s.account_id, s.currency, s.effective))
        .collect();
    assert_eq!(expected_tuples.len(), expected_count, "no duplicate tuples");

    // Page through with a small page size; the union must match exactly,
    // with no duplicates or gaps across cursor boundaries.
    let mut collected = Vec::new();
    let mut after = None;
    let mut pages = 0;
    loop {
        let page = cala
            .balances()
            .effective()
            .list_modified_since(
                journal.id(),
                since,
                es_entity::PaginatedQueryArgs { first: 3, after },
            )
            .await?;
        pages += 1;
        assert!(page.entities.len() <= 3);
        let has_next = page.has_next_page;
        after = page.end_cursor;
        collected.extend(page.entities);
        if !has_next {
            break;
        }
    }
    assert!(pages > 1, "test setup should require multiple pages");
    let paginated_tuples: HashSet<(AccountId, Currency, NaiveDate)> = collected
        .iter()
        .map(|s| (s.account_id, s.currency, s.effective))
        .collect();
    assert_eq!(paginated_tuples, expected_tuples);
    assert_eq!(
        collected.len(),
        expected_count,
        "no duplicates across pages"
    );

    // Single-row page (first == 1), then resume exactly at that boundary.
    let first_page = cala
        .balances()
        .effective()
        .list_modified_since(
            journal.id(),
            since,
            es_entity::PaginatedQueryArgs {
                first: 1,
                after: None,
            },
        )
        .await?;
    assert_eq!(first_page.entities.len(), 1);
    assert!(first_page.has_next_page);
    let boundary_tuple = (
        first_page.entities[0].account_id,
        first_page.entities[0].currency,
        first_page.entities[0].effective,
    );

    let rest = cala
        .balances()
        .effective()
        .list_modified_since(
            journal.id(),
            since,
            es_entity::PaginatedQueryArgs {
                first: expected_count,
                after: first_page.end_cursor,
            },
        )
        .await?;
    assert!(!rest.has_next_page);
    assert_eq!(rest.entities.len(), expected_count - 1);
    assert!(
        rest.entities
            .iter()
            .all(|s| (s.account_id, s.currency, s.effective) != boundary_tuple),
        "the boundary tuple must not repeat on the next page"
    );

    // No rows once the watermark moves past all activity.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let quiet_since = Utc::now();
    let empty = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), quiet_since, all_balances_query())
        .await?;
    assert!(empty.entities.is_empty());
    assert!(!empty.has_next_page);
    assert!(empty.end_cursor.is_none());

    Ok(())
}

/// A tuple whose only activity is before the watermark must be absent, even
/// while a sibling tuple touched after the watermark is present.
#[tokio::test]
async fn list_modified_since_excludes_untouched_tuples() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
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

    let (sender, _) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let (_, untouched_receiver) = helpers::test_accounts();
    let untouched_account = cala.accounts().create(untouched_receiver).await?;
    let (_, touched_receiver) = helpers::test_accounts();
    let touched_account = cala.accounts().create(touched_receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let date = NaiveDate::from_ymd_opt(2025, 11, 1).unwrap();

    // The untouched account's only activity is before the watermark.
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", untouched_account.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let since = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", touched_account.id());
    params.insert("effective", date);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let page = cala
        .balances()
        .effective()
        .list_modified_since(journal.id(), since, all_balances_query())
        .await?;

    let touched_ids: HashSet<AccountId> = page.entities.iter().map(|s| s.account_id).collect();
    assert!(touched_ids.contains(&touched_account.id()));
    assert!(
        !touched_ids.contains(&untouched_account.id()),
        "a tuple whose last change is before the watermark must be absent"
    );

    Ok(())
}
