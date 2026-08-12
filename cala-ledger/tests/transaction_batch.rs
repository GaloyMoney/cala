//! Batch posting: ordering, atomicity and the invariants that let concurrent
//! batches overlap.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    account::NewAccount, balance::error::BalanceError, error::LedgerError, posting::PostingInput,
    tx_template::*, *,
};

async fn init() -> anyhow::Result<CalaLedger> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    Ok(CalaLedger::init(cala_config, &mut jobs).await?)
}

fn transfer(
    code: &str,
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
) -> PostingInput {
    let mut params = Params::new();
    params.insert("journal_id", journal_id.to_string());
    params.insert("sender", sender);
    params.insert("recipient", recipient);
    PostingInput::new(TransactionId::new(), code, params)
}

/// Each posting in the template debits the sender 1290 BTC and credits the
/// recipient the same, so a batch of `n` must move exactly `n * 1290`.
const BTC_PER_POSTING: i64 = 1290;

#[tokio::test]
async fn batch_of_postings_on_overlapping_accounts_chains_balances() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, receiver) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(receiver).await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    // Every posting hits the same two accounts, so the fold has to chain
    // snapshot versions within the batch rather than write one per posting from
    // the same base.
    let batch: Vec<_> = (0..5)
        .map(|_| transfer(&code, journal.id(), sender.id(), recipient.id()))
        .collect();
    let posted = cala.post_transactions(batch).await?;
    assert_eq!(posted.len(), 5);

    let balance = cala
        .balances()
        .find(journal.id(), sender.id(), "BTC".parse().unwrap())
        .await?;
    assert_eq!(
        balance.details.settled.dr_balance,
        Decimal::from(5 * BTC_PER_POSTING)
    );
    // Five postings x two BTC entries against this account: the snapshot chain
    // advanced once per entry, not once for the batch.
    assert_eq!(balance.details.version, 5);

    // Every posting's entries are readable and attached to their transaction.
    for tx in posted.iter() {
        let entries = cala.entries().list_for_transaction_id(tx.id()).await?;
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].values().sequence, 1);
    }
    Ok(())
}

#[tokio::test]
async fn batch_result_matches_the_same_postings_run_one_at_a_time() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let (a, b) = helpers::test_accounts();
    let (batched_sender, batched_recipient) = (
        cala.accounts().create(a).await?,
        cala.accounts().create(b).await?,
    );
    let (c, d) = helpers::test_accounts();
    let (looped_sender, looped_recipient) = (
        cala.accounts().create(c).await?,
        cala.accounts().create(d).await?,
    );

    cala.post_transactions(
        (0..4)
            .map(|_| {
                transfer(
                    &code,
                    journal.id(),
                    batched_sender.id(),
                    batched_recipient.id(),
                )
            })
            .collect(),
    )
    .await?;

    for _ in 0..4 {
        cala.post_transactions(vec![transfer(
            &code,
            journal.id(),
            looped_sender.id(),
            looped_recipient.id(),
        )])
        .await?;
    }

    for currency in ["BTC", "USD"] {
        let currency = currency.parse().unwrap();
        let batched = cala
            .balances()
            .find(journal.id(), batched_sender.id(), currency)
            .await?;
        let looped = cala
            .balances()
            .find(journal.id(), looped_sender.id(), currency)
            .await?;
        assert_eq!(
            batched.details.settled.dr_balance,
            looped.details.settled.dr_balance
        );
        assert_eq!(batched.details.version, looped.details.version);
        assert_eq!(
            batched.details.pending.dr_balance,
            looped.details.pending.dr_balance
        );
    }
    Ok(())
}

#[tokio::test]
async fn batch_spans_journals_and_templates() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal_a = cala.journals().create(helpers::test_journal()).await?;
    let journal_b = cala.journals().create(helpers::test_journal()).await?;

    let code_a = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let code_b = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code_a))
        .await?;
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code_b))
        .await?;

    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    // The same accounts in two journals, via two templates, in one batch: the
    // fold must key balances per journal or these would collide.
    let posted = cala
        .post_transactions(vec![
            transfer(&code_a, journal_a.id(), sender.id(), recipient.id()),
            transfer(&code_b, journal_b.id(), sender.id(), recipient.id()),
            transfer(&code_a, journal_a.id(), sender.id(), recipient.id()),
        ])
        .await?;
    assert_eq!(posted.len(), 3);

    let btc = "BTC".parse().unwrap();
    let in_a = cala
        .balances()
        .find(journal_a.id(), sender.id(), btc)
        .await?;
    let in_b = cala
        .balances()
        .find(journal_b.id(), sender.id(), btc)
        .await?;
    assert_eq!(
        in_a.details.settled.dr_balance,
        Decimal::from(2 * BTC_PER_POSTING)
    );
    assert_eq!(
        in_b.details.settled.dr_balance,
        Decimal::from(BTC_PER_POSTING)
    );
    Ok(())
}

#[tokio::test]
async fn a_rejected_posting_rolls_back_the_whole_batch() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    // A good posting, then one naming an account that does not exist.
    let ghost = AccountId::new();
    let result = cala
        .post_transactions(vec![
            transfer(&code, journal.id(), sender.id(), recipient.id()),
            transfer(&code, journal.id(), sender.id(), ghost),
        ])
        .await;

    match result {
        Err(LedgerError::Posting(err)) => {
            assert_eq!(err.index, 1, "the second posting is the offender");
        }
        Err(other) => panic!("expected an attributed posting rejection, got {other:?}"),
        Ok(_) => panic!("expected the batch to be rejected"),
    }

    // Nothing landed — not even the posting that was fine on its own.
    assert!(
        cala.balances()
            .find(journal.id(), sender.id(), "BTC".parse().unwrap())
            .await
            .is_err(),
        "the valid posting in the batch must have been rolled back too"
    );
    Ok(())
}

#[tokio::test]
async fn a_locked_account_rejects_its_batch() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    let mut locked = cala.accounts().find(sender.id()).await?;
    let _ = locked.update_status(Status::Locked);
    cala.accounts().persist(&mut locked).await?;

    let result = cala
        .post_transactions(vec![transfer(
            &code,
            journal.id(),
            sender.id(),
            recipient.id(),
        )])
        .await;
    assert!(
        matches!(
            result,
            Err(LedgerError::BalanceError(
                    BalanceError::AccountLocked(id)
            )) if id == sender.id()
        ),
        "expected AccountLocked, got {:?}",
        result.err()
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_transaction_ids_within_a_batch_are_rejected() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    // The same id twice would violate the primary key inside the apply
    // statement, where it is not attributable to a posting; catching it during
    // preparation keeps the error pointed at the input that caused it.
    let mut first = transfer(&code, journal.id(), sender.id(), recipient.id());
    let mut second = transfer(&code, journal.id(), sender.id(), recipient.id());
    let shared = TransactionId::new();
    first.tx_id = shared;
    second.tx_id = shared;

    let result = cala.post_transactions(vec![first, second]).await;
    match result {
        Err(LedgerError::Posting(err)) => assert_eq!(err.index, 1),
        Err(other) => panic!("expected a duplicate-id rejection, got {other:?}"),
        Ok(_) => panic!("expected the batch to be rejected"),
    }
    Ok(())
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() -> anyhow::Result<()> {
    let cala = init().await?;
    assert!(cala.post_transactions(Vec::new()).await?.is_empty());
    Ok(())
}

/// Overlapping concurrent batches used to be able to deadlock: the old
/// per-posting loop acquired balance locks across posting boundaries with no
/// global ordering. The flow now takes one canonically sorted union lock batch,
/// so batches drawing from a shared account pool must simply serialise.
#[tokio::test]
async fn concurrent_overlapping_batches_do_not_deadlock() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let mut accounts = Vec::new();
    for i in 0..6 {
        let suffix = Alphanumeric.sample_string(&mut rand::rng(), 24);
        accounts.push(
            cala.accounts()
                .create(
                    NewAccount::builder()
                        .id(AccountId::new())
                        .name(format!("batch deadlock probe {i}"))
                        .code(format!("BATCH-DEADLOCK-{i}-{suffix}"))
                        .build()
                        .unwrap(),
                )
                .await?
                .id(),
        );
    }

    // Each worker walks the shared pool in a different rotation, so the raw
    // (account, currency) sets overlap in conflicting input orders. Only the
    // flow's canonical sort makes this safe.
    let mut handles = Vec::new();
    for worker in 0..6usize {
        let cala = cala.clone();
        let code = code.clone();
        let accounts = accounts.clone();
        let journal_id = journal.id();
        handles.push(tokio::spawn(async move {
            for round in 0..4usize {
                let batch: Vec<_> = (0..3)
                    .map(|i| {
                        let s = (worker + round + i) % accounts.len();
                        let r = (worker + round + i + 1 + worker) % accounts.len();
                        let r = if r == s { (r + 1) % accounts.len() } else { r };
                        transfer(&code, journal_id, accounts[s], accounts[r])
                    })
                    .collect();
                cala.post_transactions(batch).await?;
            }
            Ok::<(), LedgerError>(())
        }));
    }
    for handle in handles {
        handle.await?.expect("no batch may fail or deadlock");
    }

    // 6 workers x 4 rounds x 3 postings, each moving BTC_PER_POSTING out of one
    // account and into another: the pool's debits and credits must balance.
    let btc = "BTC".parse().unwrap();
    let mut total_dr = Decimal::ZERO;
    let mut total_cr = Decimal::ZERO;
    for account_id in accounts {
        let balance = cala.balances().find(journal.id(), account_id, btc).await?;
        total_dr += balance.details.settled.dr_balance;
        total_cr += balance.details.settled.cr_balance;
    }
    assert_eq!(total_dr, total_cr);
    assert_eq!(total_dr, Decimal::from(6 * 4 * 3 * BTC_PER_POSTING));
    Ok(())
}
