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
    account_set::{AccountSet, AccountSetId, NewAccountSet},
    balance::error::BalanceError,
    error::LedgerError,
    job::Jobs,
    journal::NewJournal,
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

/// #802 guard: a direct entry to an account-set backing account is rejected —
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
            matches!(&result, Err(LedgerError::EntryTargetsAccountSet)),
            "posting to set-backing account {set_account} must be rejected, got {:?}",
            result.err(),
        );
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
