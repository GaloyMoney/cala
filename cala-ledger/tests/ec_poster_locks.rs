//! Poster advisory-lock behaviour under the attach fence.
//!
//! The posting flow takes the class-1 (`EC_SET_LOCK_CLASS`) SHARED
//! membership-guard lock on every distinct **entry account** — EC and
//! non-EC alike — *before* inserting its first entry row
//! (`Balances::lock_entry_balances_in_op`), and holds it to commit.
//! Ancestor account sets are never locked in that namespace: the
//! membership guard's EXCLUSIVE is taken on the *member* being
//! attached, so leaf locks are all the fence needs.
//!
//! Advisory locks are inspected via `pg_locks` while the posting
//! operation's transaction is still open, so the test needs its own
//! database (`helpers::init_isolated_pool`) — classid-1 locks from
//! concurrently running tests in a shared database would pollute the
//! observation.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{
    account::*, account_set::NewAccountSet, primitives::BalanceRollup, tx_template::Params, *,
};

async fn init_cala(pool: sqlx::PgPool) -> anyhow::Result<CalaLedger> {
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    Ok(CalaLedger::init(cala_config, &mut jobs).await?)
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

/// Count all advisory locks in the membership-guard namespace (classid
/// `EC_SET_LOCK_CLASS` = 1) held in this database.
async fn classid_1_advisory_locks(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM pg_locks
        WHERE locktype = 'advisory'
        AND classid = 1
        AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Count the classid-1 advisory locks held on a *specific* account —
/// `objid` is the unsigned form of `hashtext(<account id>)`, the same
/// key the lock preludes use. `shared_only` restricts to SHARED holds.
async fn classid_1_locks_on(
    pool: &sqlx::PgPool,
    id: impl Into<AccountId>,
    shared_only: bool,
) -> anyhow::Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM pg_locks
        WHERE locktype = 'advisory'
        AND classid = 1
        AND objid::bigint = (hashtext($1)::bigint & 4294967295)
        AND (NOT $2 OR mode = 'ShareLock')
        AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
        "#,
    )
    .bind(id.into().to_string())
    .bind(shared_only)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

#[tokio::test]
async fn posting_holds_shared_guard_locks_on_entry_accounts_never_ancestors() -> anyhow::Result<()>
{
    let pool = helpers::init_isolated_pool().await?;
    let cala = init_cala(pool.clone()).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;

    let sender = cala
        .accounts()
        .create(new_account("sync sender", BalanceRollup::Synchronous))
        .await?;
    let recipient = cala
        .accounts()
        .create(new_account("sync recipient", BalanceRollup::Synchronous))
        .await?;
    // A synchronous parent set over the sender: the poster fans into it
    // inline, but must never take a classid-1 lock on it.
    let parent = cala
        .account_sets()
        .create(new_set(
            journal.id(),
            "sync parent",
            BalanceRollup::Synchronous,
        ))
        .await?;
    cala.account_sets()
        .add_member(parent.id(), sender.id())
        .await?;

    let mut op = cala.begin_operation().await?;
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sender.id());
    params.insert("recipient", recipient.id());
    params.insert("amount", dec!(5));
    cala.post_transaction_in_op(&mut op, TransactionId::new(), &tx_code, params)
        .await?;

    assert_eq!(
        classid_1_locks_on(&pool, sender.id(), true).await?,
        1,
        "the poster must hold the SHARED guard lock on each entry account"
    );
    assert_eq!(
        classid_1_locks_on(&pool, recipient.id(), true).await?,
        1,
        "the poster must hold the SHARED guard lock on each entry account"
    );
    assert_eq!(
        classid_1_locks_on(&pool, parent.id(), false).await?,
        0,
        "ancestor sets must never be locked in the guard namespace"
    );
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        2,
        "a posting must hold exactly one guard lock per distinct entry account"
    );
    op.commit().await?;
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        0,
        "transaction-scoped locks must be gone after commit"
    );

    Ok(())
}

#[tokio::test]
async fn ec_only_posting_holds_exactly_its_leaf_locks() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let cala = init_cala(pool.clone()).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;

    let ec_sender = cala
        .accounts()
        .create(new_account(
            "ec sender",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;
    let ec_recipient = cala
        .accounts()
        .create(new_account(
            "ec recipient",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;
    // An EC ancestor over the sender: rolled up asynchronously, and the
    // poster must not lock it either.
    let ec_parent = cala
        .account_sets()
        .create(new_set(
            journal.id(),
            "ec parent",
            BalanceRollup::EventuallyConsistent,
        ))
        .await?;
    cala.account_sets()
        .add_member(ec_parent.id(), ec_sender.id())
        .await?;

    let mut op = cala.begin_operation().await?;
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", ec_sender.id());
    params.insert("recipient", ec_recipient.id());
    params.insert("amount", dec!(5));
    cala.post_transaction_in_op(&mut op, TransactionId::new(), &tx_code, params)
        .await?;

    assert_eq!(
        classid_1_locks_on(&pool, ec_sender.id(), true).await?,
        1,
        "an EC leaf is an entry account: the poster must hold its SHARED guard lock"
    );
    assert_eq!(
        classid_1_locks_on(&pool, ec_recipient.id(), true).await?,
        1,
        "an EC leaf is an entry account: the poster must hold its SHARED guard lock"
    );
    assert_eq!(
        classid_1_locks_on(&pool, ec_parent.id(), false).await?,
        0,
        "EC ancestor sets must never be locked in the guard namespace"
    );
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        2,
        "an EC-only posting must hold exactly its leaf locks"
    );
    op.commit().await?;
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        0,
        "transaction-scoped locks must be gone after commit"
    );

    Ok(())
}
