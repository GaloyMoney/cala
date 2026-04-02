mod helpers;

use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{account_set::NewAccountSet, tx_template::*, *};

#[tokio::test]
async fn transaction_post_with_effective_balances() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

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
async fn ec_account_set_effective_balance_recalculation() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

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

    // Inline set — effective balances updated immediately on post
    let inline_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("Inline Set")
        .journal_id(journal.id())
        .build()
        .unwrap();
    let inline_set = cala.account_sets().create(inline_set).await.unwrap();

    // EC set — effective balances only appear after recalculate
    let ec_set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name("EC Set")
        .journal_id(journal.id())
        .eventually_consistent(true)
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

    // --- Post 2 transactions on different effective dates ---
    let date1 = NaiveDate::from_ymd_opt(2025, 3, 10).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2025, 3, 20).unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", date1);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", date2);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    // Inline set should already have effective balances
    let inline_d1 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(inline_d1.settled(), dec!(1290));

    let inline_d2 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(inline_d2.settled(), dec!(2580));

    // EC set should have NO effective balance before recalculation
    assert!(
        cala.balances()
            .effective()
            .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date2)
            .await
            .is_err(),
        "EC set should not have effective balance before recalculation"
    );

    // --- Recalculate ---
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    // EC set effective balances should now match inline at each date
    let ec_d1 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(
        ec_d1.settled(),
        inline_d1.settled(),
        "BTC at date1 mismatch"
    );

    let ec_d2 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(
        ec_d2.settled(),
        inline_d2.settled(),
        "BTC at date2 mismatch"
    );

    // Also verify USD settled + pending
    let inline_usd_d2 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::USD, date2)
        .await?;
    let ec_usd_d2 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::USD, date2)
        .await?;
    assert_eq!(
        inline_usd_d2.settled(),
        ec_usd_d2.settled(),
        "USD settled at date2"
    );
    assert_eq!(
        inline_usd_d2.pending(),
        ec_usd_d2.pending(),
        "USD pending at date2"
    );

    // --- Idempotency: recalculate again should be a no-op ---
    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();
    let ec_d2_again = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(
        ec_d2.settled(),
        ec_d2_again.settled(),
        "should be idempotent"
    );

    // --- Incremental: post another transaction, recalculate again ---
    let date3 = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    params.insert("effective", date3);
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    cala.account_sets()
        .recalculate_balances(ec_set.id())
        .await
        .unwrap();

    // date1 cumulative should still be 1290 (only 1 tx at or before date1)
    let ec_d1_after = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date1)
        .await?;
    let inline_d1_after = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date1)
        .await?;
    assert_eq!(ec_d1_after.settled(), inline_d1_after.settled());

    // date3 cumulative should be 2580 (date1 + date3)
    let ec_d3 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date3)
        .await?;
    let inline_d3 = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date3)
        .await?;
    assert_eq!(ec_d3.settled(), inline_d3.settled());

    // date2 cumulative should be 3870 (all 3 txs)
    let ec_d2_final = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), ec_set.id(), Currency::BTC, date2)
        .await?;
    let inline_d2_final = cala
        .balances()
        .effective()
        .find_cumulative(journal.id(), inline_set.id(), Currency::BTC, date2)
        .await?;
    assert_eq!(ec_d2_final.settled(), inline_d2_final.settled());

    Ok(())
}
