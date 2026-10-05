mod helpers;

use cala_ledger::{
    account::{
        error::{AccountCodeAlreadyExists, AccountNotFound, CreateAccountRejection},
        NewAccount,
    },
    errlanes::{Fail, FatalKind, Fault},
    journal::error::{JournalCodeNotFound, JournalNotFound},
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
        Err(Fail::Rejected(CreateAccountRejection::DuplicateId(attempted))) if attempted == id));

    let duplicate_code = NewAccount::builder()
        .id(AccountId::new())
        .name("another")
        .code("lanes-code")
        .build()?;
    assert!(matches!(cala.accounts().create(duplicate_code).await,
        Err(Fail::Rejected(CreateAccountRejection::CodeAlreadyExists(AccountCodeAlreadyExists(Some(code))))) if code == "lanes-code"));

    let absent = AccountId::new();
    assert!(matches!(cala.accounts().find(absent).await,
        Err(Fail::Rejected(AccountNotFound(missing))) if missing == absent));
    // Bulk reads represent absence as data, and their signature cannot reject.
    let bulk: Result<_, CalaFault> = cala
        .accounts()
        .find_all::<cala_ledger::account::Account>(&[absent])
        .await;
    assert!(bulk?.is_empty());

    let absent_journal = cala_ledger::JournalId::new();
    assert!(matches!(cala.journals().find(absent_journal).await,
        Err(Fail::Rejected(JournalNotFound(missing))) if missing == absent_journal));
    assert!(
        matches!(cala.journals().find_by_code("absent".into()).await,
        Err(Fail::Rejected(JournalCodeNotFound(code))) if code == "absent")
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

#[tokio::test]
async fn account_set_creation_rejects_at_the_backing_insert() -> anyhow::Result<()> {
    use cala_ledger::{
        account_set::{error::CreateAccountSetRejection, NewAccountSet},
        AccountSetId,
    };
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let id = AccountSetId::new();
    let set = || {
        NewAccountSet::builder()
            .id(id)
            .name("set")
            .balance_rollup(cala_ledger::primitives::BalanceRollup::Synchronous)
            .journal_id(journal.id())
            .build()
            .unwrap()
    };
    cala.account_sets().create(set()).await?;
    assert!(matches!(cala.account_sets().create(set()).await,
        Err(Fail::Rejected(CreateAccountSetRejection::BackingDuplicateId(conflict))) if conflict == AccountId::from(id)));
    assert!(matches!(cala.account_sets().create_all(vec![set()]).await,
        Err(Fail::Rejected(CreateAccountSetRejection::BackingDuplicateId(conflict))) if conflict == AccountId::from(id)));

    let plain_id = AccountSetId::new();
    cala.accounts()
        .create(
            NewAccount::builder()
                .id(plain_id)
                .name("plain")
                .code("plain")
                .build()?,
        )
        .await?;
    let set = NewAccountSet::builder()
        .id(plain_id)
        .name("set")
        .balance_rollup(cala_ledger::primitives::BalanceRollup::Synchronous)
        .journal_id(journal.id())
        .build()?;
    assert!(matches!(cala.account_sets().create(set).await,
        Err(Fail::Rejected(CreateAccountSetRejection::BackingDuplicateId(conflict))) if conflict == AccountId::from(plain_id)));

    let code_id = AccountSetId::new();
    cala.accounts()
        .create(
            NewAccount::builder()
                .id(AccountId::new())
                .name("code collision")
                .code(code_id.to_string())
                .build()?,
        )
        .await?;
    let set = NewAccountSet::builder()
        .id(code_id)
        .name("set")
        .balance_rollup(cala_ledger::primitives::BalanceRollup::Synchronous)
        .journal_id(journal.id())
        .build()?;
    assert!(matches!(cala.account_sets().create(set).await,
        Err(Fail::Rejected(CreateAccountSetRejection::BackingCodeAlreadyExists(Some(code)))) if code == code_id.to_string()));
    Ok(())
}

#[tokio::test]
async fn velocity_attachment_distinguishes_lookup_from_required_post_insert_hydration(
) -> anyhow::Result<()> {
    use cala_ledger::{
        velocity::{
            error::{AttachVelocityControlRejection, LimitAlreadyAddedToControl},
            *,
        },
        VelocityControlId, VelocityLimitId,
    };
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
    let missing = VelocityControlId::new();
    assert!(
        matches!(cala.velocities().attach_control_to_account(missing, AccountId::new(), Params::new()).await,
        Err(Fail::Rejected(AttachVelocityControlRejection::ControlNotFound(id))) if id == missing)
    );
    let control = cala
        .velocities()
        .create_control(
            NewVelocityControl::builder()
                .id(VelocityControlId::new())
                .name("control")
                .description("contract fixture")
                .build()?,
        )
        .await?;
    let limit = cala
        .velocities()
        .create_limit(
            NewVelocityLimit::builder()
                .id(VelocityLimitId::new())
                .name("limit")
                .description("contract fixture")
                .window(vec![])
                .limit(NewLimit::builder().balance(vec![]).build()?)
                .build()?,
        )
        .await?;
    cala.velocities()
        .add_limit_to_control(control.id(), limit.id())
        .await?;
    assert!(matches!(
        cala.velocities()
            .add_limit_to_control(control.id(), limit.id())
            .await,
        Err(Fail::Rejected(LimitAlreadyAddedToControl))
    ));

    let second = cala
        .velocities()
        .create_limit(
            NewVelocityLimit::builder()
                .id(VelocityLimitId::new())
                .name("second")
                .description("contract fixture")
                .window(vec![])
                .limit(NewLimit::builder().balance(vec![]).build()?)
                .build()?,
        )
        .await?;
    // The FK still proves the control row exists. Missing event history is a
    // required hydration failure, never the requested-ID rejection above.
    sqlx::query("DELETE FROM cala_velocity_control_events WHERE id = $1")
        .bind(control.id())
        .execute(&pool)
        .await?;
    assert!(matches!(
        cala.velocities()
            .add_limit_to_control(control.id(), second.id())
            .await,
        Err(Fail::Fatal(_))
    ));
    Ok(())
}
