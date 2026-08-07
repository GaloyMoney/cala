//! Attach-fence regression tests: membership mutations vs in-flight
//! postings.
//!
//! The posting flow SHARED-locks its distinct entry accounts *before*
//! inserting its first entry row and *before* reading the account-set
//! mappings (`Balances::lock_entry_accounts_in_op`); the membership
//! guard (`MemberHasBalanceHistory`) takes EXCLUSIVE on the member
//! being attached/removed. Together they close two races:
//!
//! - **Race A (EC members):** an attach used to slip past an in-flight
//!   *first* posting to an EC account — posters held no lock on EC
//!   accounts, so the guard's `EXISTS` ran against an empty committed
//!   state while entries sat uncommitted in the poster's transaction,
//!   and pre-join activity later folded into the parent.
//! - **Race B (synchronous parents):** an attach could commit between
//!   the poster's mappings read and its (late) lock acquisition, so the
//!   poster fanned out over stale mappings and the new parent silently
//!   missed its entries — permanent drift on a synchronous set.
//!
//! Both tests fail without the fence (the EC attach never blocks; the
//! synchronous parent's balance misses the concurrent posting).
//!
//! Membership ops hold coarse/per-member advisory locks while a
//! transaction is deliberately held open here, so each test gets its
//! own database (`helpers::init_isolated_pool`) — which the streaming
//! rollup (a global outbox consumer) needs anyway.

mod helpers;

use std::time::Duration;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{
    account::*,
    account_set::{error::AccountSetError, NewAccountSet},
    primitives::BalanceRollup,
    tx_template::Params,
    *,
};

/// How long a lock-blocked task must remain pending before we accept
/// that it is truly waiting (and not just slow).
const MUST_STILL_BE_PENDING: Duration = Duration::from_millis(300);

async fn init_cala(pool: sqlx::PgPool) -> anyhow::Result<(CalaLedger, job::Jobs)> {
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    Ok((cala, jobs))
}

fn new_account(name: &str, rollup: BalanceRollup) -> NewAccount {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewAccount::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .code(code)
        .balance_rollup(rollup)
        .build()
        .unwrap()
}

fn new_set(journal_id: JournalId, name: &str, rollup: BalanceRollup) -> NewAccountSet {
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    NewAccountSet::builder()
        .id(uuid::Uuid::now_v7())
        .name(format!("{name} {code}"))
        .journal_id(journal_id)
        .balance_rollup(rollup)
        .build()
        .unwrap()
}

async fn create_template(cala: &CalaLedger) -> anyhow::Result<String> {
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;
    Ok(tx_code)
}

fn posting_params(
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
    amount: rust_decimal::Decimal,
) -> Params {
    let mut params = Params::new();
    params.insert("journal_id", journal_id.to_string());
    params.insert("sender", sender);
    params.insert("recipient", recipient);
    params.insert("amount", amount);
    params
}

/// Race A, poster first: an attach of an EC account with an in-flight
/// *first* posting must block on the poster's SHARED entry-account lock
/// and, once the poster commits, see the committed entries and reject
/// with `MemberHasBalanceHistory`. Without the fence the attach sails
/// through (posters held no lock on EC accounts) and pre-join activity
/// folds into the parent.
#[tokio::test]
async fn attach_blocks_on_in_flight_first_ec_posting_then_rejects() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;

    let ec_x = cala
        .accounts()
        .create(new_account("ec x", BalanceRollup::EventuallyConsistent))
        .await?;
    let ec_y = cala
        .accounts()
        .create(new_account("ec y", BalanceRollup::EventuallyConsistent))
        .await?;
    let ec_set = cala
        .account_sets()
        .create(new_set(
            journal.id(),
            "race-a set",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;

    // First-ever posting to X, transaction held open: entries inserted,
    // uncommitted; the fence's SHARED lock on X is held.
    let mut op = cala.begin_operation().await?;
    cala.post_transaction_in_op(
        &mut op,
        TransactionId::new(),
        &tx_code,
        posting_params(journal.id(), ec_x.id(), ec_y.id(), dec!(5)),
    )
    .await?;

    // The attach must BLOCK: its guard takes EXCLUSIVE on X, which
    // waits on the poster's SHARED.
    let cala2 = cala.clone();
    let set_id = ec_set.id();
    let member_id = ec_x.id();
    let mut blocked =
        tokio::spawn(async move { cala2.account_sets().add_member(set_id, member_id).await });
    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "attach of an account with an in-flight first posting must block on the fence"
    );

    // Poster commits -> the guard's EXISTS now sees the committed
    // entries -> the attach is correctly rejected.
    op.commit().await?;
    match blocked.await? {
        Err(AccountSetError::MemberHasBalanceHistory { .. }) => {}
        Err(other) => panic!("expected MemberHasBalanceHistory, got {other:?}"),
        Ok(_) => panic!("attach must be rejected once the first posting has committed"),
    }

    Ok(())
}

/// Race A, attach first: a poster starting while an attach of one of
/// its entry accounts is open must block at the fence *before*
/// inserting entries or reading mappings, and resume post-commit as
/// post-join activity — the streaming rollup folds it into the new EC
/// parent. Without the fence the poster never blocks and its activity
/// races the attach's guard check.
#[tokio::test]
async fn poster_blocks_on_open_ec_attach_then_folds_into_new_parent() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let (cala, mut jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let ec_x = cala
        .accounts()
        .create(new_account("ec x", BalanceRollup::EventuallyConsistent))
        .await?;
    let ec_y = cala
        .accounts()
        .create(new_account("ec y", BalanceRollup::EventuallyConsistent))
        .await?;
    let ec_set = cala
        .account_sets()
        .create(new_set(
            journal.id(),
            "race-a-reverse set",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;

    // Attach of X held open: the guard's EXCLUSIVE on X is held.
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, ec_set.id(), ec_x.id())
        .await?;

    // The poster must BLOCK at the fence prelude, before any entry
    // insert and before the mappings read.
    let cala2 = cala.clone();
    let params = posting_params(journal.id(), ec_y.id(), ec_x.id(), dec!(5));
    let code = tx_code.clone();
    let mut blocked = tokio::spawn(async move {
        cala2
            .post_transaction(TransactionId::new(), &code, params)
            .await
    });
    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "a posting to an account with an open attach must block on the fence"
    );

    // Attach commits -> the poster resumes as post-join activity.
    op.commit().await?;
    blocked.await??;

    // The rollup folds the (post-join) entries into the new parent.
    jobs.start_poll().await?;
    helpers::wait_for_settled(&cala, journal.id(), ec_set.id(), usd, dec!(5)).await?;

    Ok(())
}

/// Race B (synchronous parents, pre-existing): a poster that starts
/// while an attach to a *synchronous* set is open must block *before*
/// its mappings read, so on resume it fans inline into the new parent.
/// Without the fence the poster reads mappings first and only blocks at
/// its late balance-lock acquisition — resuming with stale mappings and
/// permanently dropping the new parent's share (this final assertion
/// fails without the fence).
#[tokio::test]
async fn poster_blocks_on_open_synchronous_attach_then_fans_into_new_parent() -> anyhow::Result<()>
{
    let pool = helpers::init_isolated_pool().await?;
    let (cala, _jobs) = init_cala(pool).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = create_template(&cala).await?;
    let usd = "USD".parse::<Currency>()?;

    let sync_x = cala
        .accounts()
        .create(new_account("sync x", BalanceRollup::Synchronous))
        .await?;
    let sync_y = cala
        .accounts()
        .create(new_account("sync y", BalanceRollup::Synchronous))
        .await?;
    let sync_set = cala
        .account_sets()
        .create(new_set(
            journal.id(),
            "race-b set",
            BalanceRollup::Synchronous,
        ))
        .await?;

    // Attach of X to the synchronous set held open.
    let mut op = cala.begin_operation().await?;
    cala.account_sets()
        .add_member_in_op(&mut op, sync_set.id(), sync_x.id())
        .await?;

    // The poster must block at the fence prelude — before the mappings
    // read, not merely before its balance writes.
    let cala2 = cala.clone();
    let params = posting_params(journal.id(), sync_y.id(), sync_x.id(), dec!(5));
    let code = tx_code.clone();
    let mut blocked = tokio::spawn(async move {
        cala2
            .post_transaction(TransactionId::new(), &code, params)
            .await
    });
    assert!(
        tokio::time::timeout(MUST_STILL_BE_PENDING, &mut blocked)
            .await
            .is_err(),
        "a posting to an account with an open attach must block on the fence"
    );

    op.commit().await?;
    blocked.await??;

    // Synchronous sets are maintained inline by the poster: the resumed
    // posting must have fanned into the newly-attached parent.
    let parent_balance = cala
        .balances()
        .find(journal.id(), sync_set.id(), usd)
        .await?;
    assert_eq!(
        parent_balance.settled(),
        dec!(5),
        "the synchronous parent must include the concurrent posting's entries"
    );

    Ok(())
}
