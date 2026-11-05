mod helpers;

use std::collections::HashMap;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{account::Account, journal::Journal, tx_template::*, *};

#[tokio::test]
async fn transaction_post() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
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

    let tx = cala
        .post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();

    let entries = cala
        .entries()
        .find_all(&tx.values().entry_ids)
        .await
        .unwrap();

    // Only one of the entries should have metadata.
    for entry in entries.values() {
        if let Some(metadata) = &entry.values().metadata {
            let metadata: HashMap<String, AccountId> =
                serde_json::from_value(metadata.clone()).unwrap();
            assert_eq!(metadata.get("sender"), Some(&sender_account.id()));
            break;
        }
    }

    // Run it again to test balance updates
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender_account.id());
    params.insert("recipient", recipient_account.id());
    cala.post_transaction(TransactionId::new(), &tx_code, params)
        .await
        .unwrap();
    let recipient_balance = cala
        .balances()
        .find(journal.id(), recipient_account.id(), "BTC".parse().unwrap())
        .await?;
    assert_eq!(recipient_balance.settled(), Decimal::from(1290 * 2));
    let all_balances = cala
        .balances()
        .find_all(&[
            (journal.id(), recipient_account.id(), "BTC".parse().unwrap()),
            (journal.id(), sender_account.id(), "BTC".parse().unwrap()),
        ])
        .await?;
    let sender_balance = all_balances
        .get(&(journal.id(), sender_account.id(), "BTC".parse().unwrap()))
        .unwrap();
    assert_eq!(sender_balance.settled(), Decimal::from(-1290 * 2));

    Ok(())
}

async fn conditional_setup(
    conditions: [Option<&str>; 4],
) -> anyhow::Result<(CalaLedger, String, Journal, Account, Account)> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, receiver) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let receiver = cala.accounts().create(receiver).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::conditional_template(&code, conditions))
        .await?;
    Ok((cala, code, journal, sender, receiver))
}

fn conditional_params(
    journal: &Journal,
    sender: &Account,
    receiver: &Account,
    flag: bool,
) -> Params {
    let mut params = Params::new();
    params.insert("journal_id", journal.id());
    params.insert("sender", sender.id());
    params.insert("recipient", receiver.id());
    params.insert("flag", flag);
    params
}

#[tokio::test]
async fn entry_condition_selects_entry_shape_with_gapless_sequences() -> anyhow::Result<()> {
    let (cala, code, journal, sender, receiver) = conditional_setup([
        Some("params.flag"),
        Some("params.flag"),
        Some("!params.flag"),
        Some("!params.flag"),
    ])
    .await?;

    for (flag, expected) in [
        (true, ["SHAPE_A_DR", "SHAPE_A_CR"]),
        (false, ["SHAPE_B_DR", "SHAPE_B_CR"]),
    ] {
        let tx = cala
            .post_transaction(
                TransactionId::new(),
                &code,
                conditional_params(&journal, &sender, &receiver, flag),
            )
            .await?;
        let entries = cala.entries().find_all(&tx.values().entry_ids).await?;
        let mut written: Vec<_> = entries
            .values()
            .map(|e| (e.values().sequence, e.values().entry_type.clone()))
            .collect();
        written.sort();
        assert_eq!(
            written,
            vec![(1, expected[0].to_string()), (2, expected[1].to_string())]
        );
    }
    Ok(())
}

#[tokio::test]
async fn entry_condition_one_sided_is_rejected_as_unbalanced() -> anyhow::Result<()> {
    use cala_ledger::posting::{PostingRejection, PreparePostingRejection};

    let (cala, code, journal, sender, receiver) =
        conditional_setup([Some("true"), Some("false"), Some("false"), Some("false")]).await?;
    let tx_id = TransactionId::new();
    let res = cala
        .post_transaction(
            tx_id,
            &code,
            conditional_params(&journal, &sender, &receiver, true),
        )
        .await;
    assert!(matches!(
        res,
        Err(cala_ledger::errlanes::Fail::Rejected(PostingRejection::Prepare(
            PreparePostingRejection::UnbalancedTransaction { posting, .. }
        ))) if posting.tx_id == tx_id
    ));
    Ok(())
}

#[tokio::test]
async fn entry_condition_non_bool_is_rejected_as_cel_conversion() -> anyhow::Result<()> {
    use cala_ledger::posting::{PostingRejection, PreparePostingRejection};

    let (cala, code, journal, sender, receiver) =
        conditional_setup([Some("'yes'"), None, None, None]).await?;
    let res = cala
        .post_transaction(
            TransactionId::new(),
            &code,
            conditional_params(&journal, &sender, &receiver, true),
        )
        .await;
    assert!(matches!(
        res,
        Err(cala_ledger::errlanes::Fail::Rejected(PostingRejection::Prepare(
            PreparePostingRejection::Cel { source, .. }
        ))) if matches!(*source, cel_interpreter::CelConversionRejection::CoreTypeCoercion(_))
    ));
    Ok(())
}
