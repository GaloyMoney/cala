//! Batch posting: ordering, atomicity and the invariants that let concurrent
//! batches overlap.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    account::NewAccount,
    account_set::{AccountSetId, NewAccountSet},
    error::LedgerError,
    posting::{PostingError, PostingInput, RejectionReason},
    tx_template::*,
    velocity::*,
    *,
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

/// Every cumulative-effective row for one account, in `all_time_version` order.
/// Compared wholesale so a batched run has to reproduce the sequential run's
/// versions and dates, not merely its final balance.
async fn effective_rows(
    cala: &CalaLedger,
    journal_id: JournalId,
    account_id: AccountId,
) -> anyhow::Result<Vec<(chrono::NaiveDate, i32, i32, String)>> {
    Ok(sqlx::query_as::<_, (chrono::NaiveDate, i32, i32, String)>(
        "SELECT effective, version, all_time_version, \
                (values->'settled'->>'dr_balance') \
         FROM cala_cumulative_effective_balances \
         WHERE journal_id = $1 AND account_id = $2 AND currency = 'BTC' \
         ORDER BY all_time_version",
    )
    .bind(uuid::Uuid::from(journal_id))
    .bind(uuid::Uuid::from(account_id))
    .fetch_all(cala.pool())
    .await?)
}

async fn velocity_rows(
    cala: &CalaLedger,
    journal_id: JournalId,
    account_id: AccountId,
) -> anyhow::Result<Vec<(i32, String)>> {
    Ok(sqlx::query_as::<_, (i32, String)>(
        "SELECT version, (values->'settled'->>'dr_balance') \
         FROM cala_velocity_balance_history \
         WHERE journal_id = $1 AND account_id = $2 ORDER BY version",
    )
    .bind(uuid::Uuid::from(journal_id))
    .bind(uuid::Uuid::from(account_id))
    .fetch_all(cala.pool())
    .await?)
}

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
        Err(LedgerError::PostingError(PostingError::Rejected { index, .. })) => {
            assert_eq!(index, 1, "the second posting is the offender");
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
            &result,
            Err(LedgerError::PostingError(PostingError::Rejected { reason, .. }))
                if matches!(reason.as_ref(), RejectionReason::AccountLocked(id) if *id == sender.id())
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
        Err(LedgerError::PostingError(PostingError::Rejected { index, .. })) => {
            assert_eq!(index, 1)
        }
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

/// The effective-balance pass is grouped by `(journal, effective date)` rather
/// than run per posting. This is the equivalence that licenses the grouping:
/// the rows a batch produces — **including `all_time_version`**, which is a
/// dense positional counter over the sorted union and is load-bearing for
/// `find_in_range`'s version diffs — must match the rows the same postings
/// produce one at a time, with effective dates deliberately out of order so
/// back-dating replay is exercised.
#[tokio::test]
async fn effective_balances_match_sequential_across_mixed_dates() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala
        .journals()
        .create(helpers::test_journal_with_effective_balances())
        .await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    // Dates deliberately unsorted: d2 back-dates behind d3, so a later posting
    // in the batch lands before an earlier one and forces the replay to shift
    // already-written future rows.
    let d1 = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
    let d2 = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let d3 = chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
    let dates = [d1, d2, d3, d2, d1];

    let dated = |sender: AccountId, recipient: AccountId, effective: chrono::NaiveDate| {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender);
        params.insert("recipient", recipient);
        params.insert("effective", effective);
        PostingInput::new(TransactionId::new(), &code, params)
    };

    let (a, b) = helpers::test_accounts();
    let (batched_s, batched_r) = (
        cala.accounts().create(a).await?,
        cala.accounts().create(b).await?,
    );
    let (c, d) = helpers::test_accounts();
    let (looped_s, looped_r) = (
        cala.accounts().create(c).await?,
        cala.accounts().create(d).await?,
    );

    cala.post_transactions(
        dates
            .iter()
            .map(|e| dated(batched_s.id(), batched_r.id(), *e))
            .collect(),
    )
    .await?;
    for e in dates.iter() {
        cala.post_transactions(vec![dated(looped_s.id(), looped_r.id(), *e)])
            .await?;
    }

    // Compare the full cumulative-effective row sets, not just the final
    // balance: effective, version and all_time_version all have to line up.
    let batched = effective_rows(&cala, journal.id(), batched_s.id()).await?;
    let looped = effective_rows(&cala, journal.id(), looped_s.id()).await?;
    assert!(!batched.is_empty(), "the batch must have written rows");
    assert_eq!(
        batched, looped,
        "batched effective rows must match sequential exactly, all_time_version included"
    );
    Ok(())
}

/// Velocity balances are collected across the whole batch into one lock/read/
/// write. A key touched by several postings must end up with the same chained
/// snapshots the same postings produce one at a time.
#[tokio::test]
async fn velocity_balances_match_sequential_across_a_batch() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::velocity_template(&code))
        .await?;

    let velocity_params = |sender: AccountId, recipient: AccountId| {
        let mut params = Params::new();
        params.insert("journal_id", journal.id().to_string());
        params.insert("sender", sender);
        params.insert("recipient", recipient);
        params.insert("amount", Decimal::from(100));
        params.insert("currency", "BTC".to_string());
        params.insert("layer", "SETTLED".to_string());
        PostingInput::new(TransactionId::new(), &code, params)
    };

    // A limit high enough that four postings never trip it — the point here is
    // the chained balance, not the rejection.
    let limit = cala
        .velocities()
        .create_limit(
            NewVelocityLimit::builder()
                .id(VelocityLimitId::new())
                .name("batch probe")
                .description("never trips")
                .window(vec![])
                .limit(
                    NewLimit::builder()
                        .balance(vec![NewBalanceLimit::builder()
                            .layer("SETTLED")
                            .amount("decimal('1000000000')")
                            .enforcement_direction("DEBIT")
                            .always_active()
                            .build()
                            .unwrap()])
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .await?;
    let control = cala
        .velocities()
        .create_control(
            NewVelocityControl::builder()
                .id(VelocityControlId::new())
                .name("batch probe")
                .description("batch probe")
                .build()
                .unwrap(),
        )
        .await?;
    cala.velocities()
        .add_limit_to_control(control.id(), limit.id())
        .await?;

    let mut made = Vec::new();
    for _ in 0..2 {
        let (s, r) = helpers::test_accounts();
        let s = cala.accounts().create(s).await?;
        let r = cala.accounts().create(r).await?;
        cala.velocities()
            .attach_control_to_account(control.id(), s.id(), Params::new())
            .await?;
        made.push((s.id(), r.id()));
    }
    let (batched_s, batched_r) = made[0];
    let (looped_s, looped_r) = made[1];

    cala.post_transactions(
        (0..4)
            .map(|_| velocity_params(batched_s, batched_r))
            .collect(),
    )
    .await?;
    for _ in 0..4 {
        cala.post_transactions(vec![velocity_params(looped_s, looped_r)])
            .await?;
    }

    let batched = velocity_rows(&cala, journal.id(), batched_s).await?;
    let looped = velocity_rows(&cala, journal.id(), looped_s).await?;
    assert!(
        !batched.is_empty(),
        "velocity history must have been written"
    );
    assert_eq!(
        batched, looped,
        "batched velocity snapshots must match sequential exactly"
    );
    Ok(())
}

/// A batch's cost in advisory locks scales with *distinct accounts*, not batch
/// size, and those locks live in Postgres' shared lock table until commit.
/// Exceeding it yields a bare `out of shared memory` that names neither cause
/// nor fix, so the flow refuses up front instead.
#[tokio::test]
async fn a_batch_touching_too_many_accounts_is_refused_with_a_clear_error() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    // Two accounts per posting, each posting on a fresh pair; the template
    // books two currencies per account, so 400 postings is 1600 distinct
    // (account, currency) balances — past the bound without being a large batch.
    let mut ids = Vec::new();
    let mut pending = Vec::new();
    for i in 0..800 {
        let id = AccountId::new();
        ids.push(id);
        pending.push(
            NewAccount::builder()
                .id(id)
                .name(format!("lock ceiling {i}"))
                .code(format!("LOCKCEIL-{}-{i}", uuid::Uuid::now_v7().simple()))
                .build()
                .unwrap(),
        );
    }
    cala.accounts().create_all(pending).await?;

    let batch: Vec<_> = (0..400)
        .map(|i| transfer(&code, journal.id(), ids[i * 2], ids[i * 2 + 1]))
        .collect();

    match cala.post_transactions(batch).await {
        Err(LedgerError::PostingError(PostingError::BatchTooManyAccounts { distinct, max })) => {
            assert!(distinct > max, "{distinct} should exceed {max}");
        }
        Err(other) => panic!("expected BatchTooManyAccounts, got {other:?}"),
        Ok(_) => panic!("expected the batch to be refused"),
    }

    // The same postings split into batches that respect the bound all land.
    for chunk in (0..400).collect::<Vec<_>>().chunks(100) {
        let batch: Vec<_> = chunk
            .iter()
            .map(|i| transfer(&code, journal.id(), ids[i * 2], ids[i * 2 + 1]))
            .collect();
        cala.post_transactions(batch).await?;
    }
    Ok(())
}

/// The template cache is optimistic: preparation uses the cached body and the
/// fence statement re-checks the version. When the version moved, the flow
/// must re-resolve from the DATABASE — not the cache, which is exactly what
/// was reported stale — and re-prepare against the fresh body.
#[tokio::test]
async fn a_stale_template_cache_is_refreshed_via_the_fence_version_check() -> anyhow::Result<()> {
    let cala = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, receiver) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(receiver).await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    // Warm this process's template cache with version 1.
    let first = cala
        .post_transactions(vec![transfer(
            &code,
            journal.id(),
            sender.id(),
            recipient.id(),
        )])
        .await?
        .pop()
        .expect("one transaction");
    assert_eq!(first.values().description, None);

    // Simulate a template update landing after preparation: a version-2 event
    // whose body pins the transaction description to a literal.
    let template_id = cala.tx_templates().find_by_code(&code).await?.id();
    sqlx::query(
        "INSERT INTO cala_tx_template_events (id, sequence, event_type, event) \
         SELECT id, 2, event_type, \
                jsonb_set(event, '{values,transaction,description}', to_jsonb($2::text)) \
         FROM cala_tx_template_events WHERE id = $1 AND sequence = 1",
    )
    .bind(uuid::Uuid::from(template_id))
    .bind("'refreshed-body'") // a CEL string literal
    .execute(cala.pool())
    .await?;

    // The cache still holds version 1; the fence must report it stale and the
    // flow must re-prepare with the refreshed body.
    let second = cala
        .post_transactions(vec![transfer(
            &code,
            journal.id(),
            sender.id(),
            recipient.id(),
        )])
        .await?
        .pop()
        .expect("one transaction");
    assert_eq!(
        second.values().description.as_deref(),
        Some("refreshed-body")
    );
    Ok(())
}

/// Outbox events must be grouped **per posting** — each `TransactionCreated`
/// immediately followed by that transaction's own `EntryCreated` events — which
/// is the interleaving a sequence of single-posting calls produces. A consumer
/// that accumulates a transaction's entries until the next `TransactionCreated`
/// would silently break if a batch ever emitted all transactions first and then
/// all entries.
#[tokio::test]
async fn outbox_events_are_grouped_per_posting_not_by_type() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;
    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    let high_water: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) FROM cala_persistent_outbox_events")
            .fetch_one(&pool)
            .await?;

    let posted = cala
        .post_transactions(
            (0..3)
                .map(|_| transfer(&code, journal.id(), sender.id(), recipient.id()))
                .collect(),
        )
        .await?;

    // (payload type, owning transaction id) in outbox sequence order.
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT payload->>'type', \
                COALESCE(payload->'transaction'->>'id', payload->'entry'->>'transaction_id') \
         FROM cala_persistent_outbox_events WHERE sequence > $1 ORDER BY sequence",
    )
    .bind(high_water)
    .fetch_all(&pool)
    .await?;

    // Build the expected sequence directly from what was posted: each
    // transaction, then exactly its own entries, in posting order.
    let mut expected: Vec<(String, String)> = Vec::new();
    for tx in posted.iter() {
        let tx_id = tx.id().to_string();
        expected.push(("transaction_created".to_string(), tx_id.clone()));
        let entries = cala.entries().list_for_transaction_id(tx.id()).await?;
        for _ in 0..entries.len() {
            expected.push(("entry_created".to_string(), tx_id.clone()));
        }
    }

    assert_eq!(
        rows, expected,
        "outbox must interleave per posting (tx, its entries, tx, its entries, ...), \
         not group all transactions before all entries"
    );
    Ok(())
}

/// A leaf may belong to account sets in more than one journal. When a batch
/// spans those journals, each posting must fan into **only** the sets belonging
/// to its own journal — otherwise a journal-B set acquires a balance row under
/// journal A, which is both wrong and unlocked (the ancestor lock batch only
/// covers the correct pairs).
#[tokio::test]
async fn a_multi_journal_batch_does_not_cross_ancestor_sets_between_journals() -> anyhow::Result<()>
{
    let cala = init().await?;
    let journal_a = cala.journals().create(helpers::test_journal()).await?;
    let journal_b = cala.journals().create(helpers::test_journal()).await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    let (a, b) = helpers::test_accounts();
    let sender = cala.accounts().create(a).await?;
    let recipient = cala.accounts().create(b).await?;

    let mut set_in = |journal_id, name: &str| {
        let s = NewAccountSet::builder()
            .id(AccountSetId::new())
            .name(name.to_string())
            .journal_id(journal_id)
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        s
    };
    let set_a = cala
        .account_sets()
        .create(set_in(journal_a.id(), "SET A"))
        .await?;
    let set_b = cala
        .account_sets()
        .create(set_in(journal_b.id(), "SET B"))
        .await?;

    // The same leaves are members of a set in each journal.
    for set in [set_a.id(), set_b.id()] {
        for member in [sender.id(), recipient.id()] {
            cala.account_sets().add_member(set, member).await?;
        }
    }

    // One posting per journal, in a single batch.
    cala.post_transactions(vec![
        transfer(&code, journal_a.id(), sender.id(), recipient.id()),
        transfer(&code, journal_b.id(), sender.id(), recipient.id()),
    ])
    .await?;

    let btc: Currency = "BTC".parse().unwrap();
    // Each set carries its own journal's posting...
    let a_in_a = cala
        .balances()
        .find(journal_a.id(), set_a.id(), btc)
        .await?;
    let b_in_b = cala
        .balances()
        .find(journal_b.id(), set_b.id(), btc)
        .await?;
    assert_eq!(
        a_in_a.details.settled.dr_balance,
        Decimal::from(BTC_PER_POSTING)
    );
    assert_eq!(
        b_in_b.details.settled.dr_balance,
        Decimal::from(BTC_PER_POSTING)
    );

    // ...and nothing at all in the other journal.
    for (journal_id, set_id, label) in [
        (journal_a.id(), set_b.id(), "set B under journal A"),
        (journal_b.id(), set_a.id(), "set A under journal B"),
    ] {
        let stray = cala.balances().find(journal_id, set_id, btc).await;
        assert!(
            stray.is_err(),
            "{label} must have no balance row, got {:?}",
            stray.ok().map(|b| b.details.settled.dr_balance)
        );
    }
    Ok(())
}
