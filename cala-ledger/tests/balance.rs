mod helpers;

use std::collections::{HashMap, HashSet};

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{account_set::NewAccountSet, balance::AccountBalance, tx_template::*, *};

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

#[tokio::test]
async fn list_current_balances_for_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, None).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let balances = cala
        .balances()
        .list_for_account(journal.id(), recipient_account.id(), all_balances_query())
        .await?;
    let balances = balances_by_currency(balances);
    let currencies: HashSet<_> = balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let btc = cala
        .balances()
        .find(journal.id(), recipient_account.id(), Currency::BTC)
        .await?;
    assert_eq!(balances[&Currency::BTC].balance_type, btc.balance_type);
    assert_eq!(balances[&Currency::BTC].details, btc.details);

    let usd = cala
        .balances()
        .find(journal.id(), recipient_account.id(), Currency::USD)
        .await?;
    assert_eq!(balances[&Currency::USD].balance_type, usd.balance_type);
    assert_eq!(balances[&Currency::USD].details, usd.details);

    let fresh = cala.accounts().create(helpers::test_accounts().0).await?;
    let empty = cala
        .balances()
        .list_for_account(journal.id(), fresh.id(), all_balances_query())
        .await?;
    let empty = balances_by_currency(empty);
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn list_current_balances_for_accounts() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, None).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_account = cala.accounts().create(receiver).await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let expected_ids = [
        (journal.id(), recipient_account.id(), Currency::BTC),
        (journal.id(), recipient_account.id(), Currency::USD),
        (journal.id(), sender_account.id(), Currency::BTC),
        (journal.id(), sender_account.id(), Currency::USD),
    ];
    let expected = cala.balances().find_all(&expected_ids).await?;

    let actual = cala
        .balances()
        .list_for_accounts(
            journal.id(),
            &[recipient_account.id(), sender_account.id()],
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
async fn list_current_balances_for_account_set() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, None).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await?;
    let recipient_one = cala.accounts().create(receiver).await?;
    let (_, receiver_two) = helpers::test_accounts();
    let recipient_two = cala.accounts().create(receiver_two).await?;

    let account_set = cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("Recipient Set")
                .journal_id(journal.id())
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;
    cala.account_sets()
        .add_member(account_set.id(), recipient_one.id())
        .await?;
    cala.account_sets()
        .add_member(account_set.id(), recipient_two.id())
        .await?;

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&tx_code))
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_one.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_two.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let balances = cala
        .balances()
        .list_for_account(
            journal.id(),
            AccountId::from(account_set.id()),
            all_balances_query(),
        )
        .await?;
    let balances = balances_by_currency(balances);
    let currencies: HashSet<_> = balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let btc = cala
        .balances()
        .find(journal.id(), account_set.id(), Currency::BTC)
        .await?;
    assert_eq!(balances[&Currency::BTC].balance_type, btc.balance_type);
    assert_eq!(balances[&Currency::BTC].details, btc.details);

    let usd = cala
        .balances()
        .find(journal.id(), account_set.id(), Currency::USD)
        .await?;
    assert_eq!(balances[&Currency::USD].balance_type, usd.balance_type);
    assert_eq!(balances[&Currency::USD].details, usd.details);

    let recipient_one_btc = cala
        .balances()
        .find(journal.id(), recipient_one.id(), Currency::BTC)
        .await?;
    let recipient_two_btc = cala
        .balances()
        .find(journal.id(), recipient_two.id(), Currency::BTC)
        .await?;
    assert_balance_amounts_sum(
        &balances[&Currency::BTC],
        &recipient_one_btc,
        &recipient_two_btc,
    );

    let recipient_one_usd = cala
        .balances()
        .find(journal.id(), recipient_one.id(), Currency::USD)
        .await?;
    let recipient_two_usd = cala
        .balances()
        .find(journal.id(), recipient_two.id(), Currency::USD)
        .await?;
    assert_balance_amounts_sum(
        &balances[&Currency::USD],
        &recipient_one_usd,
        &recipient_two_usd,
    );

    let account_set_id = AccountId::from(&account_set.id());
    let expected_ids = [
        (journal.id(), account_set_id, Currency::BTC),
        (journal.id(), account_set_id, Currency::USD),
    ];
    let expected = cala.balances().find_all(&expected_ids).await?;
    let actual = cala
        .balances()
        .list_for_accounts(journal.id(), &[account_set_id], all_balances_query())
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
async fn list_current_balances_for_eventually_consistent_account_set() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, Some(&mut jobs)).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;

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

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_one.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_two.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await?;

    let ec_before_rollup = cala
        .balances()
        .list_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            all_balances_query(),
        )
        .await?;
    let ec_before_rollup = balances_by_currency(ec_before_rollup);
    assert!(ec_before_rollup.is_empty());

    // The streaming rollup populates the EC set asynchronously.
    jobs.start_poll().await?;

    let inline_balances = cala
        .balances()
        .list_for_account(
            journal.id(),
            AccountId::from(inline_set.id()),
            all_balances_query(),
        )
        .await?;
    let inline_balances = balances_by_currency(inline_balances);
    for currency in [Currency::BTC, Currency::USD] {
        helpers::wait_for_settled(
            &cala,
            journal.id(),
            ec_set.id(),
            currency,
            inline_balances[&currency].settled(),
        )
        .await?;
    }

    let ec_balances = cala
        .balances()
        .list_for_account(
            journal.id(),
            AccountId::from(ec_set.id()),
            all_balances_query(),
        )
        .await?;
    let ec_balances = balances_by_currency(ec_balances);
    let currencies: HashSet<_> = ec_balances.keys().copied().collect();
    assert_eq!(currencies, HashSet::from([Currency::BTC, Currency::USD]));

    let ec_btc = cala
        .balances()
        .find(journal.id(), ec_set.id(), Currency::BTC)
        .await?;
    assert_eq!(
        ec_balances[&Currency::BTC].balance_type,
        ec_btc.balance_type
    );
    assert_eq!(ec_balances[&Currency::BTC].details, ec_btc.details);

    let ec_usd = cala
        .balances()
        .find(journal.id(), ec_set.id(), Currency::USD)
        .await?;
    assert_eq!(
        ec_balances[&Currency::USD].balance_type,
        ec_usd.balance_type
    );
    assert_eq!(ec_balances[&Currency::USD].details, ec_usd.details);

    for currency in [Currency::BTC, Currency::USD] {
        assert_balance_amounts_eq(&ec_balances[&currency], &inline_balances[&currency]);
    }

    Ok(())
}
