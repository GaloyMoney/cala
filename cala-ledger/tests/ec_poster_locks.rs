//! Poster advisory-lock behaviour on eventually-consistent accounts.
//!
//! `BalanceRepo::find_for_update` gates both of its per-row advisory
//! locks — the SHARED membership-guard fence (classid
//! `EC_SET_LOCK_CLASS` = 1) and the per-balance FOR_UPDATE lock — on
//! `NOT eventually_consistent`. A posting that touches only EC nodes
//! must therefore hold no classid-1 advisory locks at all, while a
//! posting touching non-EC accounts must still hold them.
//!
//! Advisory locks are inspected via `pg_locks` while the posting
//! operation's transaction is still open, so the test needs its own
//! database (`helpers::init_isolated_pool`) — classid-1 locks from
//! concurrently running tests in a shared database would pollute the
//! observation.

mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal_macros::dec;

use cala_ledger::{account::*, primitives::BalanceRollup, tx_template::Params, *};

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

/// Count the advisory locks in the membership-guard namespace (classid
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

#[tokio::test]
async fn ec_only_posting_holds_no_membership_guard_locks() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let cala = init_cala(pool.clone()).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::simple_template_with_date_default(&tx_code))
        .await?;

    // Control: a posting between two synchronous accounts must hold the
    // SHARED membership-guard lock on both — proves the observation
    // itself works.
    let sync_sender = cala
        .accounts()
        .create(new_account("sync sender", BalanceRollup::Synchronous))
        .await?;
    let sync_recipient = cala
        .accounts()
        .create(new_account("sync recipient", BalanceRollup::Synchronous))
        .await?;

    let mut op = cala.begin_operation().await?;
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", sync_sender.id());
    params.insert("recipient", sync_recipient.id());
    params.insert("amount", dec!(5));
    cala.post_transaction_in_op(&mut op, TransactionId::new(), &tx_code, params)
        .await?;
    assert!(
        classid_1_advisory_locks(&pool).await? >= 2,
        "a posting touching synchronous accounts must hold the SHARED guard lock on them"
    );
    op.commit().await?;
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        0,
        "transaction-scoped locks must be gone after commit"
    );

    // A posting that touches only eventually-consistent nodes must
    // acquire no classid-1 advisory locks at all.
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

    let mut op = cala.begin_operation().await?;
    let mut params = Params::new();
    params.insert("journal_id", journal.id().to_string());
    params.insert("sender", ec_sender.id());
    params.insert("recipient", ec_recipient.id());
    params.insert("amount", dec!(5));
    cala.post_transaction_in_op(&mut op, TransactionId::new(), &tx_code, params)
        .await?;
    assert_eq!(
        classid_1_advisory_locks(&pool).await?,
        0,
        "a posting touching only EC nodes must hold no membership-guard advisory locks"
    );
    op.commit().await?;

    Ok(())
}
