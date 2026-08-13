//! End-to-end tests for the streaming EC account-set balance rollup job.
//!
//! These exercise the obix `OutboxEventHandler` job that consumes the
//! outbox and folds each committed transaction's leaf deltas into its
//! ancestor EC account sets.
//!
//! The rollup job is a **global** outbox consumer, so every test here runs
//! on its own isolated database (`helpers::init_isolated_pool`).
//!
//! The rollup is registered inside `CalaLedger::init` (we pass a `Jobs` we
//! own). Each test posts its whole workload **before** calling
//! `jobs.start_poll()` and then waits for convergence. This "backlog" shape
//! is a race-free proxy for catch-up correctness: it deterministically
//! verifies the rollup (collecting transactions, loading entries, computing
//! EC-ancestor deltas, materializing snapshots, checkpoint advancement)
//! without interleaving posters against the live listener — the
//! poster-vs-listener concurrency belongs to obix, not to the rollup logic
//! under test here.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use cala_ledger::{
    account::{Account, NewAccount},
    account_set::{error::AccountSetError, AccountSet, AccountSetId, NewAccountSet},
    balance::error::BalanceError,
    error::LedgerError,
    job::Jobs,
    journal::NewJournal,
    posting::{PostingError, RejectionReason},
    primitives::BalanceRollup,
    tx_template::Params,
    AccountId, CalaLedger, CalaLedgerConfig, Currency, JournalId, TransactionId,
};

const N_MEMBERS: usize = 4;
const POST_AMOUNT: Decimal = dec!(7);

struct Fixture {
    cala: CalaLedger,
    journal_id: JournalId,
    sender: Account,
    members: Vec<Account>,
    tx_code: String,
}

/// Build a fixture whose ledger has the rollup registered against a `Jobs`
/// we return to the caller (unstarted — the caller polls it after posting).
async fn setup(pool: sqlx::PgPool, journal: NewJournal) -> anyhow::Result<(Fixture, Jobs)> {
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(journal).await?;

    let sender_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let sender = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("Streaming rollup sender {sender_code}"))
        .code(sender_code)
        .build()?;
    let sender = cala.accounts().create(sender).await?;

    let mut members = Vec::with_capacity(N_MEMBERS);
    for i in 0..N_MEMBERS {
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let acc = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Streaming rollup member {i} {code}"))
            .code(code)
            .build()?;
        members.push(cala.accounts().create(acc).await?);
    }

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;

    let journal_id = journal.id();
    Ok((
        Fixture {
            cala,
            journal_id,
            sender,
            members,
            tx_code,
        },
        jobs,
    ))
}

async fn create_ec_set(
    cala: &CalaLedger,
    journal_id: JournalId,
    name: &str,
) -> anyhow::Result<AccountSet> {
    let set = NewAccountSet::builder()
        .id(AccountSetId::new())
        .name(name)
        .journal_id(journal_id)
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()?;
    Ok(cala.account_sets().create(set).await?)
}

/// A **plain** (non-set) account opted into eventually-consistent balance
/// maintenance via the new public `balance_rollup` setter.
async fn create_ec_plain_account(cala: &CalaLedger, name: &str) -> anyhow::Result<Account> {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let account = NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .code(code)
        .balance_rollup(BalanceRollup::EventuallyConsistent)
        .build()?;
    Ok(cala.accounts().create(account).await?)
}

async fn post_round_robin(fixture: &Fixture, n_posts: usize) -> anyhow::Result<()> {
    for i in 0..n_posts {
        post_to(fixture, fixture.members[i % fixture.members.len()].id(), 1).await?;
    }
    Ok(())
}

async fn post_to(fixture: &Fixture, recipient: AccountId, n: usize) -> anyhow::Result<()> {
    for _ in 0..n {
        let mut params = Params::new();
        params.insert("journal_id", fixture.journal_id.to_string());
        params.insert("sender", fixture.sender.id());
        params.insert("recipient", recipient);
        params.insert("amount", POST_AMOUNT);
        fixture
            .cala
            .post_transaction(TransactionId::new(), &fixture.tx_code, params)
            .await?;
    }
    Ok(())
}

async fn assert_member_sum(
    fixture: &Fixture,
    currency: Currency,
    expected: Decimal,
) -> anyhow::Result<()> {
    let mut sum = Decimal::ZERO;
    for m in &fixture.members {
        match fixture
            .cala
            .balances()
            .find(fixture.journal_id, m.id(), currency)
            .await
        {
            Ok(b) => sum += b.settled(),
            Err(BalanceError::NotFound(..)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    assert_eq!(
        sum, expected,
        "sum of member balances must equal sum of posts"
    );
    Ok(())
}

/// Post the whole backlog, then start the job and assert the EC set
/// converges to the sum of all posts from the beginning of the outbox.
#[tokio::test]
async fn streaming_rollup_catches_up_from_backlog() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "backlog EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 12;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // Sanity: nothing has rolled up the EC set yet (poller not started).
    assert!(
        fixture
            .cala
            .balances()
            .find(fixture.journal_id, ec_set.id(), usd)
            .await
            .is_err(),
        "EC set must have no balance before the streaming job runs",
    );

    jobs.start_poll().await?;

    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;
    assert_member_sum(&fixture, usd, expected).await?;
    Ok(())
}

/// Nested EC sets: a leaf's delta must fan into every EC ancestor
/// (`parent_ec ⊇ child_ec ⊇ leaves`), so both converge to the same total.
#[tokio::test]
async fn streaming_rollup_fans_into_nested_ec_ancestors() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let parent_ec = create_ec_set(&fixture.cala, fixture.journal_id, "nested parent EC").await?;
    let child_ec = create_ec_set(&fixture.cala, fixture.journal_id, "nested child EC").await?;
    fixture
        .cala
        .account_sets()
        .add_member(parent_ec.id(), child_ec.id())
        .await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(child_ec.id(), m.id())
            .await?;
    }

    let n_posts = 8;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        child_ec.id(),
        usd,
        expected,
    )
    .await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        parent_ec.id(),
        usd,
        expected,
    )
    .await?;
    Ok(())
}

/// The streaming rollup must produce the *same* per-event balance history
/// for an EC set as the inline poster path produces for a non-EC set with
/// the same member — same settled balance and the same version count.
#[tokio::test]
async fn streaming_rollup_matches_inline_set_history() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let recipient = fixture.members[0].id();

    // Inline (non-EC) reference set + EC set, both holding the recipient.
    let inline_set = fixture
        .cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("inline reference set")
                .journal_id(fixture.journal_id)
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;
    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "EC vs inline set").await?;
    for set in [inline_set.id(), ec_set.id()] {
        fixture
            .cala
            .account_sets()
            .add_member(set, recipient)
            .await?;
    }

    let n_posts = 5;
    post_to(&fixture, recipient, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    // Inline set is synchronous; wait for the EC set to catch up.
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;

    let inline_bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, inline_set.id(), usd)
        .await?;
    let ec_bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(ec_bal.settled(), inline_bal.settled());
    assert_eq!(
        ec_bal.details.version, inline_bal.details.version,
        "EC set must have the same per-event version count as the inline set",
    );
    Ok(())
}

/// A single member belonging to two EC sets has its activity folded into
/// both.
#[tokio::test]
async fn streaming_rollup_shared_member_fans_into_multiple_sets() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let recipient = fixture.members[0].id();
    let set_a = create_ec_set(&fixture.cala, fixture.journal_id, "shared set A").await?;
    let set_b = create_ec_set(&fixture.cala, fixture.journal_id, "shared set B").await?;
    for set in [set_a.id(), set_b.id()] {
        fixture
            .cala
            .account_sets()
            .add_member(set, recipient)
            .await?;
    }

    let n_posts = 6;
    post_to(&fixture, recipient, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, set_a.id(), usd, expected).await?;
    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, set_b.id(), usd, expected).await?;
    Ok(())
}

/// With effective balances enabled on the journal, the streaming rollup
/// must maintain the EC set's cumulative effective balance too.
#[tokio::test]
async fn streaming_rollup_maintains_effective_balances() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal_with_effective_balances()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "effective EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 8;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    // Both the settled and cumulative-effective projections converge.
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;
    let today = fixture.cala.clock().now().date_naive();
    helpers::wait_for_effective(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        today,
        expected,
    )
    .await?;
    Ok(())
}

/// An **eventually-consistent plain account** (not a set) takes no inline
/// balance write on posting: `Balances::find` is `NotFound` until the rollup
/// runs, after which the leaf's own balance equals the summed entries.
#[tokio::test]
async fn streaming_rollup_maintains_ec_plain_account_leaf() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let leaf = create_ec_plain_account(&fixture.cala, "EC plain leaf").await?;

    let n_posts = 9;
    post_to(&fixture, leaf.id(), n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // EC leaf: nothing is written inline while the poller is stopped.
    assert!(
        matches!(
            fixture
                .cala
                .balances()
                .find(fixture.journal_id, leaf.id(), usd)
                .await,
            Err(BalanceError::NotFound(..))
        ),
        "EC plain account must have no inline balance before the rollup runs",
    );

    jobs.start_poll().await?;

    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, leaf.id(), usd, expected).await?;
    Ok(())
}

/// An EC plain leaf that is also a member of an EC set: the rollup folds each
/// entry into the leaf's own balance *and* the ancestor set, independently —
/// both converge to the same total with no double count.
#[tokio::test]
async fn streaming_rollup_folds_ec_plain_leaf_and_its_ec_set() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let leaf = create_ec_plain_account(&fixture.cala, "EC plain member leaf").await?;
    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "parent of EC leaf").await?;
    fixture
        .cala
        .account_sets()
        .add_member(ec_set.id(), leaf.id())
        .await?;

    let n_posts = 7;
    post_to(&fixture, leaf.id(), n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, leaf.id(), usd, expected).await?;
    helpers::wait_for_settled(
        &fixture.cala,
        fixture.journal_id,
        ec_set.id(),
        usd,
        expected,
    )
    .await?;
    Ok(())
}

/// With effective balances enabled, the rollup maintains an EC plain leaf's
/// cumulative-effective balance too.
#[tokio::test]
async fn streaming_rollup_maintains_ec_plain_leaf_effective_balance() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal_with_effective_balances()).await?;

    let leaf = create_ec_plain_account(&fixture.cala, "EC plain effective leaf").await?;

    let n_posts = 6;
    post_to(&fixture, leaf.id(), n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    jobs.start_poll().await?;

    helpers::wait_for_settled(&fixture.cala, fixture.journal_id, leaf.id(), usd, expected).await?;
    let today = fixture.cala.clock().now().date_naive();
    helpers::wait_for_effective(
        &fixture.cala,
        fixture.journal_id,
        leaf.id(),
        usd,
        today,
        expected,
    )
    .await?;
    Ok(())
}

/// Set-guard: a direct entry to an account-set backing account is rejected —
/// its balance is derived from members, so the entry would be folded nowhere.
/// The composite FK enforces this for **any** set, EC or synchronous.
#[tokio::test]
async fn rejects_direct_entry_to_account_set() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "guard EC set").await?;
    let sync_set = fixture
        .cala
        .account_sets()
        .create(
            NewAccountSet::builder()
                .id(AccountSetId::new())
                .name("guard sync set")
                .journal_id(fixture.journal_id)
                .balance_rollup(BalanceRollup::Synchronous)
                .build()?,
        )
        .await?;

    for set_account in [
        AccountId::from(&ec_set.id()),
        AccountId::from(&sync_set.id()),
    ] {
        let mut params = Params::new();
        params.insert("journal_id", fixture.journal_id.to_string());
        params.insert("sender", fixture.sender.id());
        params.insert("recipient", set_account);
        params.insert("amount", POST_AMOUNT);
        // `Transaction` isn't `Debug`, so match on the result by reference
        // (avoids `expect_err`'s `T: Debug` bound).
        let result = fixture
            .cala
            .post_transaction(TransactionId::new(), &fixture.tx_code, params)
            .await;
        assert!(
            matches!(
                &result,
                Err(LedgerError::PostingError(PostingError::Rejected { reason, .. }))
                    if matches!(reason.as_ref(), RejectionReason::EntryTargetsAccountSet(_))
            ),
            "posting to set-backing account {set_account} must be rejected, got {:?}",
            result.err(),
        );
    }
    Ok(())
}

/// Regression: an EC plain leaf writes balance
/// history only when the rollup runs, but its entries land synchronously. The
/// membership guard must see those entries — otherwise a leaf that has already
/// posted could still be attached to a set, and the rollup would later fold its
/// pre-membership entries into the set, corrupting the set balance.
#[tokio::test]
async fn ec_leaf_with_posted_entries_cannot_join_a_set() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let leaf = create_ec_plain_account(&fixture.cala, "EC membership-guard leaf").await?;
    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "membership guard set").await?;

    // Post to the EC leaf. The rollup is not started, so the leaf has entries
    // but no materialized balance history yet.
    post_to(&fixture, leaf.id(), 1).await?;
    assert!(
        fixture
            .cala
            .balances()
            .find(fixture.journal_id, leaf.id(), usd)
            .await
            .is_err(),
        "EC leaf must have no materialized balance before the rollup runs",
    );

    // Attaching the leaf to a set must be rejected: it already has activity
    // (entries), even though its balance history is not yet materialized.
    let add = fixture
        .cala
        .account_sets()
        .add_member(ec_set.id(), leaf.id())
        .await;
    assert!(
        matches!(add, Err(AccountSetError::MemberHasBalanceHistory { .. })),
        "EC leaf with posted entries must not be attachable to a set",
    );
    Ok(())
}

/// Regression: a direct entry to a *nonexistent* account must
/// fail as a plain referential-integrity error, NOT be misreported as targeting
/// an account set. The plain account-id FK is checked before the set-guard FK,
/// so a missing account trips it first and never reaches the set-guard mapping.
#[tokio::test]
async fn missing_account_is_not_reported_as_account_set() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let missing = AccountId::new(); // never created

    let mut params = Params::new();
    params.insert("journal_id", fixture.journal_id.to_string());
    params.insert("sender", fixture.sender.id());
    params.insert("recipient", missing);
    params.insert("amount", POST_AMOUNT);
    let result = fixture
        .cala
        .post_transaction(TransactionId::new(), &fixture.tx_code, params)
        .await;

    match result {
        Err(LedgerError::PostingError(PostingError::Rejected { reason, .. }))
            if matches!(reason.as_ref(), RejectionReason::EntryTargetsAccountSet(_)) =>
        {
            panic!("a missing account was misreported as targeting an account set")
        }
        Err(_) => {} // a referential-integrity / not-found error — correct
        Ok(_) => panic!("posting to a nonexistent account must fail"),
    }
    Ok(())
}

/// Regression: a default (Synchronous) plain account is written inline and is
/// readable immediately after posting — the rollup job is irrelevant to it.
#[tokio::test]
async fn synchronous_plain_account_is_readable_immediately() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let recipient = fixture.members[0].id();
    let n_posts = 4;
    post_to(&fixture, recipient, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // No `start_poll` — read-your-write holds for a synchronous account.
    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, recipient, usd)
        .await?;
    assert_eq!(bal.settled(), expected);
    Ok(())
}

/// The caught-up barrier: after `await_completion` returns `Ok`, every
/// transaction committed before the snapshot was taken is folded into EC
/// balances — settled *and* cumulative-effective (same commit) — and
/// readable immediately, with **no** further polling. A second fence after
/// more posts renews the guarantee (the semantics are self-renewing, not
/// one-shot).
#[tokio::test]
async fn await_completion_fences_backlog_and_renews() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal_with_effective_balances()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "barrier EC set").await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 12;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // Poller not started: the status snapshot must report honest lag.
    let status = fixture.cala.ec_rollup_status().await?;
    assert!(
        !status.is_caught_up() && status.lag() > 0,
        "rollup must lag behind a posted backlog before the poller runs, got {status:?}",
    );

    // The fence pinned above, before the poller existed, is the one awaited:
    // the frontier does not move under the wait.
    jobs.start_poll().await?;
    status
        .await_completion(std::time::Duration::from_secs(60))
        .await?;

    // The ordering guarantee: read EC balances *immediately* — no
    // wait_for_settled polling.
    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(
        bal.settled(),
        expected,
        "EC set balance must be complete immediately after the fence",
    );
    let today = fixture.cala.clock().now().date_naive();
    let effective = fixture
        .cala
        .balances()
        .effective()
        .find_cumulative(fixture.journal_id, ec_set.id(), usd, today)
        .await?;
    assert_eq!(
        effective.settled(),
        expected,
        "cumulative-effective EC balance must be complete immediately after the fence",
    );
    assert_member_sum(&fixture, usd, expected).await?;

    // The rollup's own applies published `BalanceUpdated` events *behind*
    // the fence's frontier snapshot, so a status taken right after the
    // fence may transiently report lag (a skip-only tail the checkpoint
    // crosses lazily). A second fence drains it; with no further activity
    // the status is then stably caught up.
    fixture
        .cala
        .ec_rollup_status()
        .await?
        .await_completion(std::time::Duration::from_secs(60))
        .await?;
    assert!(
        fixture.cala.ec_rollup_status().await?.is_caught_up(),
        "status must report caught-up once the self-published tail is drained",
    );

    // Self-renewing: a second backlog + second fence must again be fully
    // reflected on return.
    let more_posts = 5;
    post_round_robin(&fixture, more_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts + more_posts);
    fixture
        .cala
        .ec_rollup_status()
        .await?
        .await_completion(std::time::Duration::from_secs(60))
        .await?;
    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(
        bal.settled(),
        expected,
        "second fence must reflect the second backlog immediately",
    );
    Ok(())
}

/// A stopped (or wedged) rollup must surface as a rich, alertable
/// [`LedgerError::EcCaughtUpTimeout`] — never a silent hang. The error
/// carries the observed checkpoint and frontier so an operator can see
/// exactly how far behind the stream is.
#[tokio::test]
async fn await_completion_times_out_when_rollup_is_stalled() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, _jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(&fixture.cala, fixture.journal_id, "stalled EC set").await?;
    fixture
        .cala
        .account_sets()
        .add_member(ec_set.id(), fixture.members[0].id())
        .await?;
    post_to(&fixture, fixture.members[0].id(), 3).await?;

    // Poller never started. A zero timeout still runs one check and
    // errors immediately.
    match fixture
        .cala
        .ec_rollup_status()
        .await?
        .await_completion(std::time::Duration::ZERO)
        .await
    {
        Err(LedgerError::EcCaughtUpTimeout {
            applied, frontier, ..
        }) => {
            assert_eq!(
                u64::from(applied),
                0,
                "a never-run rollup reports checkpoint BEGIN",
            );
            assert!(applied < frontier, "frontier must be ahead of checkpoint");
        }
        other => panic!("expected EcCaughtUpTimeout, got {other:?}"),
    }

    // A nonzero timeout polls with backoff and reports how long it waited.
    let timeout = std::time::Duration::from_millis(400);
    match fixture
        .cala
        .ec_rollup_status()
        .await?
        .await_completion(timeout)
        .await
    {
        Err(LedgerError::EcCaughtUpTimeout { waited, .. }) => {
            assert!(
                waited >= timeout,
                "error must report the full wait, got {waited:?}",
            );
        }
        other => panic!("expected EcCaughtUpTimeout, got {other:?}"),
    }
    Ok(())
}

/// `await_frontier` targets a caller-supplied sequence directly, rather
/// than sampling the frontier at call time like `EcRollupStatus` does. The
/// value can be captured once and carried across a boundary the pinned
/// snapshot itself can't cross — stored, passed to another task, awaited
/// later — since only the plain `EventSequence` survives, not the
/// `EcRollupStatus` (and its handle) that produced it.
#[tokio::test]
async fn await_frontier_waits_for_a_previously_captured_sequence() -> anyhow::Result<()> {
    let usd: Currency = "USD".parse().unwrap();
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal_with_effective_balances()).await?;

    let ec_set = create_ec_set(
        &fixture.cala,
        fixture.journal_id,
        "captured-frontier EC set",
    )
    .await?;
    for m in &fixture.members {
        fixture
            .cala
            .account_sets()
            .add_member(ec_set.id(), m.id())
            .await?;
    }

    let n_posts = 8;
    post_round_robin(&fixture, n_posts).await?;
    let expected = POST_AMOUNT * Decimal::from(n_posts);

    // Capture just the plain sequence value — the `EcRollupStatus` that
    // read it is dropped immediately, proving it isn't needed again.
    let frontier = fixture.cala.ec_rollup_status().await?.frontier;

    jobs.start_poll().await?;
    fixture
        .cala
        .await_frontier(frontier, std::time::Duration::from_secs(60))
        .await?;

    let bal = fixture
        .cala
        .balances()
        .find(fixture.journal_id, ec_set.id(), usd)
        .await?;
    assert_eq!(
        bal.settled(),
        expected,
        "EC set balance must be complete once the captured frontier is reached",
    );
    assert_member_sum(&fixture, usd, expected).await?;
    Ok(())
}

/// A target beyond anything the outbox has assigned can never be
/// satisfied — unlike `await_completion`'s call-time frontier, which is
/// always reachable once applied. The timeout error must report the exact
/// sequence requested, not a resampled "current" frontier, which is the
/// property that distinguishes `await_frontier` from the snapshot-bound
/// wait.
#[tokio::test]
async fn await_frontier_times_out_for_a_sequence_beyond_the_stream() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (fixture, mut jobs) = setup(pool, helpers::test_journal()).await?;

    let ec_set = create_ec_set(
        &fixture.cala,
        fixture.journal_id,
        "unreachable-frontier EC set",
    )
    .await?;
    fixture
        .cala
        .account_sets()
        .add_member(ec_set.id(), fixture.members[0].id())
        .await?;
    post_to(&fixture, fixture.members[0].id(), 3).await?;
    jobs.start_poll().await?;

    let current = fixture.cala.ec_rollup_status().await?.frontier;
    let unreachable = obix::EventSequence::from(u64::from(current) + 1_000);

    let timeout = std::time::Duration::from_millis(300);
    match fixture.cala.await_frontier(unreachable, timeout).await {
        Err(LedgerError::EcCaughtUpTimeout {
            frontier, waited, ..
        }) => {
            assert_eq!(
                frontier, unreachable,
                "error must report the exact target requested, not a resampled frontier",
            );
            assert!(
                waited >= timeout,
                "error must report the full wait, got {waited:?}",
            );
        }
        other => panic!("expected EcCaughtUpTimeout, got {other:?}"),
    }
    Ok(())
}
