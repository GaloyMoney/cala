mod helpers;

use cala_ledger::{
    account::{error::AccountRejection, NewAccount},
    errlanes::{Fail, FatalKind, Fault},
    journal::error::JournalRejection,
    AccountId, CalaFault, CalaLedger, CalaLedgerConfig,
};

#[tokio::test]
async fn duplicates_reject_but_missing_rows_and_corrupt_events_keep_distinct_contracts(
) -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool.clone())
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;
    let id = AccountId::new();
    let new_account = || {
        NewAccount::builder()
            .id(id)
            .name("lanes")
            .code("lanes-code")
            .build()
            .unwrap()
    };
    cala.accounts().create(new_account()).await?;
    assert!(matches!(cala.accounts().create(new_account()).await,
        Err(Fail::Rejected(AccountRejection::DuplicateId(attempted))) if attempted == id));

    let duplicate_code = NewAccount::builder()
        .id(AccountId::new())
        .name("another")
        .code("lanes-code")
        .build()?;
    assert!(matches!(cala.accounts().create(duplicate_code).await,
        Err(Fail::Rejected(AccountRejection::CodeAlreadyExists(Some(code)))) if code == "lanes-code"));

    let absent = AccountId::new();
    assert!(matches!(cala.accounts().find(absent).await,
        Err(Fail::Rejected(AccountRejection::CouldNotFindById(missing))) if missing == absent));
    // Bulk reads represent absence as data, and their signature cannot reject.
    let bulk: Result<_, CalaFault> = cala
        .accounts()
        .find_all::<cala_ledger::account::Account>(&[absent])
        .await;
    assert!(bulk?.is_empty());

    let absent_journal = cala_ledger::JournalId::new();
    assert!(matches!(cala.journals().find(absent_journal).await,
        Err(Fail::Rejected(JournalRejection::CouldNotFindById(missing))) if missing == absent_journal));
    assert!(
        matches!(cala.journals().find_by_code("absent".into()).await,
        Err(Fail::Rejected(JournalRejection::CouldNotFindByCode(code))) if code == "absent")
    );

    sqlx::query("UPDATE cala_account_events SET event = '{}'::jsonb WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    let error = cala
        .accounts()
        .find(id)
        .await
        .err()
        .expect("corrupt stored event");
    assert!(matches!(error, Fail::Fatal(ref fatal) if fatal.kind == FatalKind::CorruptState));
    assert!(!matches!(error, Fail::Rejected(_)));
    Ok(())
}

#[tokio::test]
async fn a_closed_pool_is_a_fault_for_both_rejecting_and_fault_only_methods() -> anyhow::Result<()>
{
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool.clone())
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;
    pool.close().await;
    assert!(matches!(
        cala.accounts().find(AccountId::new()).await,
        Err(Fail::Transient(_))
    ));
    assert!(matches!(
        cala.begin_operation().await,
        Err(Fault::Transient(_))
    ));
    Ok(())
}
