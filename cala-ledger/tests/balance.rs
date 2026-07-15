mod helpers;

use std::collections::HashSet;

use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn find_all_current_balances_for_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

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
        .find_all_for_account(journal.id(), recipient_account.id())
        .await?;
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
        .find_all_for_account(journal.id(), fresh.id())
        .await?;
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn find_all_current_balances_for_accounts() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

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
        .find_all_for_accounts(&[
            (journal.id(), recipient_account.id()),
            (journal.id(), sender_account.id()),
        ])
        .await?;

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
