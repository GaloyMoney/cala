//! `ensure_*_in_op`: create-or-find on tx templates, velocity limits and
//! controls, accounts and account sets, plus the idempotent attach / add-member
//! variants. Each `Found` returns the stored entity and persists nothing.
mod helpers;

use cala_ledger::{
    account::{error::*, NewAccount},
    account_set::{error::*, NewAccountSet},
    errlanes::Fail,
    tx_template::error::*,
    velocity::*,
    *,
};
use es_entity::{errlanes::RejectionCode, Ensured};
use rand::distr::{Alphanumeric, SampleString};
use std::time::Duration;

async fn init() -> anyhow::Result<(CalaLedger, sqlx::PgPool)> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool.clone())
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    Ok((cala, pool))
}

fn random(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        Alphanumeric.sample_string(&mut rand::rng(), 24)
    )
}

/// Give a still-blocked concurrent task time to reach its blocking statement.
async fn let_it_block() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}

// --- catalogue pins -------------------------------------------------------

fn codes<C: RejectionCode>() -> Vec<&'static str> {
    C::CODES.iter().map(|c| c.code).collect()
}

#[test]
fn residual_rejections_are_exhaustive_and_pinned() {
    fn tx_template(r: EnsureTxTemplateRejection) {
        match r {
            EnsureTxTemplateRejection::DuplicateId(_) => {}
        }
    }
    fn account_by_id(r: EnsureAccountRejection) {
        match r {
            EnsureAccountRejection::InitialAccountSetNotFound(_)
            | EnsureAccountRejection::CodeAlreadyExists(_)
            | EnsureAccountRejection::ExternalIdAlreadyExists(_) => {}
        }
    }
    fn account_by_external_id(r: EnsureAccountByExternalIdRejection) {
        match r {
            EnsureAccountByExternalIdRejection::InitialAccountSetNotFound(_)
            | EnsureAccountByExternalIdRejection::DuplicateId(_)
            | EnsureAccountByExternalIdRejection::CodeAlreadyExists(_) => {}
        }
    }
    fn account_set(r: EnsureAccountSetByExternalIdRejection) {
        match r {
            EnsureAccountSetByExternalIdRejection::BackingDuplicateId(_)
            | EnsureAccountSetByExternalIdRejection::BackingCodeAlreadyExists(_) => {}
        }
    }
    let _ = (
        tx_template,
        account_by_id,
        account_by_external_id,
        account_set,
    );

    assert_eq!(
        codes::<EnsureTxTemplateRejectionCode>(),
        ["CALA_TX_TEMPLATE_DUPLICATE_ID"]
    );
    assert_eq!(
        <EnsureAccountRejectionCode as RejectionCode>::CODES.len(),
        3
    );
    assert_eq!(
        <EnsureAccountByExternalIdRejectionCode as RejectionCode>::CODES.len(),
        3
    );
    assert_eq!(
        codes::<EnsureAccountSetByExternalIdRejectionCode>(),
        [
            "CALA_ACCOUNT_SET_BACKING_ACCOUNT_ID_ALREADY_EXISTS",
            "CALA_ACCOUNT_SET_BACKING_ACCOUNT_CODE_ALREADY_EXISTS"
        ]
    );
}

// --- tx templates ---------------------------------------------------------

#[tokio::test]
async fn tx_template_ensure_creates_then_finds() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let code = random("ensure-tpl");

    let mut op = cala.begin_operation().await?;
    let first = cala
        .tx_templates()
        .ensure_in_op(&mut op, helpers::currency_conversion_template(&code))
        .await?;
    let second = cala
        .tx_templates()
        .ensure_in_op(&mut op, helpers::currency_conversion_template(&code))
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    assert!(!second.is_created());
    assert_eq!(first.into_inner().id(), second.into_inner().id());
    Ok(())
}

#[tokio::test]
async fn tx_template_ensure_found_returns_stored_not_new() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let code = random("ensure-tpl-stored");

    let mut op = cala.begin_operation().await?;
    let stored = cala
        .tx_templates()
        .ensure_in_op(&mut op, helpers::currency_conversion_template(&code))
        .await?
        .into_inner();
    op.commit().await?;

    // Same code, different body and a fresh id.
    let other = helpers::simple_template_with_date_default(&code);
    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala.tx_templates().ensure_in_op(&mut op, other).await? else {
        panic!("a taken code must be found");
    };
    op.commit().await?;

    assert_eq!(found.id(), stored.id());
    assert_eq!(found.values().entries.len(), stored.values().entries.len());
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cala_tx_templates WHERE code = $1")
        .bind(&code)
        .fetch_one(&pool)
        .await?;
    assert_eq!(rows, 1);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_tx_template_events e JOIN cala_tx_templates t ON t.id = e.id WHERE t.code = $1",
    )
    .bind(&code)
    .fetch_one(&pool)
    .await?;
    assert_eq!(events, 1, "Found must not persist the new events");
    Ok(())
}

#[tokio::test]
async fn tx_template_ensure_taken_id_under_another_code_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let reused_id = TxTemplateId::new();
    cala.tx_templates()
        .create(template_with_id(reused_id, &random("ensure-tpl-id-a")))
        .await?;
    let other = template_with_id(reused_id, &random("ensure-tpl-id-b"));

    let mut op = cala.begin_operation().await?;
    let err = cala
        .tx_templates()
        .ensure_in_op(&mut op, other)
        .await
        .err()
        .expect("a taken id must be rejected");
    let Fail::Rejected(EnsureTxTemplateRejection::DuplicateId(id)) = err else {
        panic!("expected DuplicateId, got {err:?}");
    };
    assert_eq!(id, reused_id);
    Ok(())
}

fn template_with_id(id: TxTemplateId, code: &str) -> cala_ledger::tx_template::NewTxTemplate {
    use cala_ledger::tx_template::*;
    NewTxTemplate::builder()
        .id(id)
        .code(code)
        .transaction(
            NewTxTemplateTransaction::builder()
                .effective("date()")
                .journal_id("uuid('00000000-0000-0000-0000-000000000000')")
                .build()
                .unwrap(),
        )
        .entries(vec![])
        .build()
        .unwrap()
}

#[tokio::test]
async fn tx_template_ensure_concurrent_creates_once_and_finds_once() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let code = random("ensure-tpl-concurrent");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .tx_templates()
        .ensure_in_op(&mut op_a, helpers::currency_conversion_template(&code))
        .await?;

    let cala_b = cala.clone();
    let code_b = code.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .tx_templates()
            .ensure_in_op(&mut op_b, helpers::currency_conversion_template(&code_b))
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished(), "B must wait for A's uncommitted insert");
    op_a.commit().await?;
    let b = b.await??;

    assert!(a.is_created());
    let Ensured::Found(found) = b else {
        panic!("the loser must find the winner's template");
    };
    assert_eq!(found.id(), a.into_inner().id());
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cala_tx_templates WHERE code = $1")
        .bind(&code)
        .fetch_one(&pool)
        .await?;
    assert_eq!(rows, 1);
    Ok(())
}

// --- velocity -------------------------------------------------------------

fn new_limit(id: VelocityLimitId, name: &str) -> NewVelocityLimit {
    NewVelocityLimit::builder()
        .id(id)
        .name(name)
        .description("test")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("decimal('100')")
                    .enforcement_direction("DEBIT")
                    .always_active()
                    .build()
                    .expect("balance limit")])
                .build()
                .expect("limit"),
        )
        .build()
        .expect("new limit")
}

fn new_control(id: VelocityControlId, name: &str) -> NewVelocityControl {
    NewVelocityControl::builder()
        .id(id)
        .name(name)
        .description("test")
        .build()
        .expect("new control")
}

#[tokio::test]
async fn velocity_limit_and_control_ensure_create_then_find_the_stored_one() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let velocity = cala.velocities();
    let limit_id = VelocityLimitId::new();
    let control_id = VelocityControlId::new();

    let mut op = cala.begin_operation().await?;
    let limit = velocity
        .ensure_limit_in_op(&mut op, new_limit(limit_id, "first"))
        .await?;
    let control = velocity
        .ensure_control_in_op(&mut op, new_control(control_id, "first"))
        .await?;
    op.commit().await?;
    assert!(limit.is_created() && control.is_created());

    let mut op = cala.begin_operation().await?;
    let Ensured::Found(limit) = velocity
        .ensure_limit_in_op(&mut op, new_limit(limit_id, "second"))
        .await?
    else {
        panic!("a taken limit id must be found");
    };
    let Ensured::Found(control) = velocity
        .ensure_control_in_op(&mut op, new_control(control_id, "second"))
        .await?
    else {
        panic!("a taken control id must be found");
    };
    op.commit().await?;

    assert_eq!(limit.values().name, "first");
    assert_eq!(control.values().name, "first");
    for (table, id) in [
        ("cala_velocity_limit_events", limit_id.to_string()),
        ("cala_velocity_control_events", control_id.to_string()),
    ] {
        let events: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id = $1::uuid"))
                .bind(id)
                .fetch_one(&pool)
                .await?;
        assert_eq!(events, 1, "Found must not persist the new events");
    }
    Ok(())
}

#[tokio::test]
async fn velocity_limit_ensure_concurrent_creates_once_and_finds_once() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let limit_id = VelocityLimitId::new();

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .velocities()
        .ensure_limit_in_op(&mut op_a, new_limit(limit_id, "a"))
        .await?;
    let cala_b = cala.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .velocities()
            .ensure_limit_in_op(&mut op_b, new_limit(limit_id, "b"))
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished());
    op_a.commit().await?;
    let b = b.await??;
    assert!(a.is_created());
    assert!(!b.is_created());
    assert_eq!(b.into_inner().values().name, "a");
    Ok(())
}

#[tokio::test]
async fn attach_limit_to_control_is_idempotent() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let velocity = cala.velocities();
    let limit = velocity
        .create_limit(new_limit(VelocityLimitId::new(), "attach"))
        .await?;
    let control = velocity
        .create_control(new_control(VelocityControlId::new(), "attach"))
        .await?;

    let mut op = cala.begin_operation().await?;
    let first = velocity
        .ensure_limit_added_to_control_in_op(&mut op, control.id(), limit.id())
        .await?;
    let second = velocity
        .ensure_limit_added_to_control_in_op(&mut op, control.id(), limit.id())
        .await?;
    // The transaction was never poisoned: it can still read and commit.
    let attached = velocity
        .list_limits_for_control_in_op(&mut op, control.id())
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    assert!(!second.is_created());
    assert_eq!(attached.len(), 1);
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_velocity_control_limits WHERE velocity_control_id = $1",
    )
    .bind(uuid::Uuid::from(control.id()))
    .fetch_one(&pool)
    .await?;
    assert_eq!(rows, 1);

    // Existing, non-idempotent behaviour is untouched.
    let res = velocity
        .add_limit_to_control(control.id(), limit.id())
        .await;
    assert!(matches!(
        res,
        Err(Fail::Rejected(
            cala_ledger::velocity::error::LimitAlreadyAddedToControl
        ))
    ));
    Ok(())
}

#[tokio::test]
async fn attach_limit_to_control_concurrent_is_found_not_a_fault() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let velocity = cala.velocities();
    let limit = velocity
        .create_limit(new_limit(VelocityLimitId::new(), "attach-c"))
        .await?;
    let control = velocity
        .create_control(new_control(VelocityControlId::new(), "attach-c"))
        .await?;
    let (control_id, limit_id) = (control.id(), limit.id());

    let mut op_a = cala.begin_operation().await?;
    let a = velocity
        .ensure_limit_added_to_control_in_op(&mut op_a, control_id, limit_id)
        .await?;
    let cala_b = cala.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .velocities()
            .ensure_limit_added_to_control_in_op(&mut op_b, control_id, limit_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        // Still usable after the conflicting attach.
        cala_b
            .velocities()
            .list_limits_for_control_in_op(&mut op_b, control_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished());
    op_a.commit().await?;
    let b = b.await??;
    assert!(a.is_created());
    assert!(!b.is_created());
    Ok(())
}

// --- accounts -------------------------------------------------------------

fn new_account(id: AccountId, code: &str, external_id: Option<&str>, name: &str) -> NewAccount {
    let mut builder = NewAccount::builder();
    builder.id(id).name(name).code(code);
    if let Some(external_id) = external_id {
        builder.external_id(external_id);
    }
    builder.build().unwrap()
}

#[tokio::test]
async fn account_ensure_by_id_creates_then_finds_the_stored_one() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let id = AccountId::new();
    let code = random("ensure-acc");

    let mut op = cala.begin_operation().await?;
    let first = cala
        .accounts()
        .ensure_in_op(&mut op, new_account(id, &code, None, "first"))
        .await?;
    // Same id; every other field differs, including the code.
    let second = cala
        .accounts()
        .ensure_in_op(&mut op, new_account(id, &random("other"), None, "second"))
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    let Ensured::Found(found) = second else {
        panic!("a taken id must be found");
    };
    assert_eq!(found.values().name, "first");
    assert_eq!(found.values().code, code);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cala_account_events WHERE id = $1")
        .bind(uuid::Uuid::from(id))
        .fetch_one(&pool)
        .await?;
    assert_eq!(events, 1);
    Ok(())
}

#[tokio::test]
async fn account_ensure_by_id_other_unique_conflict_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let code = random("ensure-acc-code");
    let external_id = random("ensure-acc-ext");
    cala.accounts()
        .create(new_account(
            AccountId::new(),
            &code,
            Some(&external_id),
            "holder",
        ))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .accounts()
        .ensure_in_op(
            &mut op,
            new_account(AccountId::new(), &code, None, "same code"),
        )
        .await
        .err()
        .expect("a code held by another account must be rejected");
    let Fail::Rejected(EnsureAccountRejection::CodeAlreadyExists(_)) = err else {
        panic!("expected CodeAlreadyExists, got {err:?}");
    };
    drop(op);

    let mut op = cala.begin_operation().await?;
    let err = cala
        .accounts()
        .ensure_in_op(
            &mut op,
            new_account(
                AccountId::new(),
                &random("c"),
                Some(&external_id),
                "same ext",
            ),
        )
        .await
        .err()
        .expect("an external id held by another account must be rejected");
    let Fail::Rejected(EnsureAccountRejection::ExternalIdAlreadyExists(_)) = err else {
        panic!("expected ExternalIdAlreadyExists, got {err:?}");
    };
    Ok(())
}

#[tokio::test]
async fn account_ensure_by_external_id_creates_then_finds_the_stored_one() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let external_id = random("ensure-acc-ext");
    let first_id = AccountId::new();

    let mut op = cala.begin_operation().await?;
    let first = cala
        .accounts()
        .ensure_by_external_id_in_op(
            &mut op,
            new_account(first_id, &random("c"), Some(&external_id), "first"),
        )
        .await?;
    let second = cala
        .accounts()
        .ensure_by_external_id_in_op(
            &mut op,
            new_account(AccountId::new(), &random("c"), Some(&external_id), "second"),
        )
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    let Ensured::Found(found) = second else {
        panic!("a taken external id must be found");
    };
    assert_eq!(found.id(), first_id);
    assert_eq!(found.values().name, "first");
    Ok(())
}

#[tokio::test]
async fn account_ensure_by_external_id_other_unique_conflict_is_a_rejection() -> anyhow::Result<()>
{
    let (cala, _) = init().await?;
    let taken = cala
        .accounts()
        .create(new_account(AccountId::new(), &random("c"), None, "holder"))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .accounts()
        .ensure_by_external_id_in_op(
            &mut op,
            new_account(taken.id(), &random("c"), Some(&random("e")), "same id"),
        )
        .await
        .err()
        .expect("an id held by another account must be rejected");
    let Fail::Rejected(EnsureAccountByExternalIdRejection::DuplicateId(id)) = err else {
        panic!("expected DuplicateId, got {err:?}");
    };
    assert_eq!(id, taken.id());
    drop(op);

    let mut op = cala.begin_operation().await?;
    let err = cala
        .accounts()
        .ensure_by_external_id_in_op(
            &mut op,
            new_account(
                AccountId::new(),
                &taken.values().code,
                Some(&random("e")),
                "same code",
            ),
        )
        .await
        .err()
        .expect("a code held by another account must be rejected");
    let Fail::Rejected(EnsureAccountByExternalIdRejection::CodeAlreadyExists(_)) = err else {
        panic!("expected CodeAlreadyExists, got {err:?}");
    };
    Ok(())
}

#[tokio::test]
async fn account_ensure_by_external_id_concurrent_creates_once_and_finds_once() -> anyhow::Result<()>
{
    let (cala, _) = init().await?;
    let external_id = random("ensure-acc-conc");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .accounts()
        .ensure_by_external_id_in_op(
            &mut op_a,
            new_account(AccountId::new(), &random("c"), Some(&external_id), "a"),
        )
        .await?;
    let cala_b = cala.clone();
    let external_b = external_id.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .accounts()
            .ensure_by_external_id_in_op(
                &mut op_b,
                new_account(AccountId::new(), &random("c"), Some(&external_b), "b"),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished());
    op_a.commit().await?;
    let b = b.await??;
    assert!(a.is_created());
    assert!(!b.is_created());
    assert_eq!(b.into_inner().id(), a.into_inner().id());
    Ok(())
}

// --- account sets ---------------------------------------------------------

fn new_set(
    id: AccountSetId,
    journal_id: JournalId,
    name: &str,
    external_id: Option<&str>,
) -> NewAccountSet {
    let mut builder = NewAccountSet::builder();
    builder
        .id(id)
        .name(name)
        .journal_id(journal_id)
        .balance_rollup(primitives::BalanceRollup::Synchronous);
    if let Some(external_id) = external_id {
        builder.external_id(external_id);
    }
    builder.build().unwrap()
}

/// Backing accounts without a set row: what a lost ensure would leave behind.
async fn orphan_backing_accounts(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_accounts a WHERE a.is_account_set AND NOT EXISTS (SELECT 1 FROM cala_account_sets s WHERE s.id = a.id)",
    )
    .fetch_one(pool)
    .await?)
}

#[tokio::test]
async fn account_set_ensure_creates_then_finds_without_a_second_backing_account(
) -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let external_id = random("ensure-set");
    let first_id = AccountSetId::new();

    let mut op = cala.begin_operation().await?;
    let first = cala
        .account_sets()
        .ensure_by_external_id_in_op(
            &mut op,
            new_set(first_id, journal.id(), "first", Some(&external_id)),
        )
        .await?;
    op.commit().await?;
    assert!(first.is_created());

    let mut op = cala.begin_operation().await?;
    let second = cala
        .account_sets()
        .ensure_by_external_id_in_op(
            &mut op,
            new_set(
                AccountSetId::new(),
                journal.id(),
                "second",
                Some(&external_id),
            ),
        )
        .await?;
    op.commit().await?;

    let Ensured::Found(found) = second else {
        panic!("a taken external id must be found");
    };
    assert_eq!(found.id(), first_id);
    assert_eq!(found.values().name, "first");
    assert_eq!(
        orphan_backing_accounts(&pool).await?,
        0,
        "Found must not leave a backing account behind"
    );
    Ok(())
}

#[tokio::test]
async fn account_set_ensure_taken_set_id_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let existing = cala
        .account_sets()
        .create(new_set(AccountSetId::new(), journal.id(), "holder", None))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .account_sets()
        .ensure_by_external_id_in_op(
            &mut op,
            new_set(existing.id(), journal.id(), "same id", Some(&random("e"))),
        )
        .await
        .err()
        .expect("a taken set id must be rejected");
    let Fail::Rejected(EnsureAccountSetByExternalIdRejection::BackingDuplicateId(id)) = err else {
        panic!("expected BackingDuplicateId, got {err:?}");
    };
    assert_eq!(id, AccountId::from(existing.id()));
    Ok(())
}

#[tokio::test]
async fn account_set_ensure_concurrent_creates_once_and_leaves_one_backing_account(
) -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let journal_id = journal.id();
    let external_id = random("ensure-set-conc");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .account_sets()
        .ensure_by_external_id_in_op(
            &mut op_a,
            new_set(AccountSetId::new(), journal_id, "a", Some(&external_id)),
        )
        .await?;
    let cala_b = cala.clone();
    let external_b = external_id.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .account_sets()
            .ensure_by_external_id_in_op(
                &mut op_b,
                new_set(AccountSetId::new(), journal_id, "b", Some(&external_b)),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished(), "B must wait for A on the external id");
    op_a.commit().await?;
    let b = b.await??;

    assert!(a.is_created());
    let Ensured::Found(found) = b else {
        panic!("the loser must find the winner's set");
    };
    assert_eq!(found.id(), a.into_inner().id());
    assert_eq!(orphan_backing_accounts(&pool).await?, 0);
    let backing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cala_account_sets WHERE external_id = $1")
            .bind(&external_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(backing, 1);
    Ok(())
}

// --- account-set membership -----------------------------------------------

async fn three_sets(
    cala: &CalaLedger,
) -> anyhow::Result<(AccountSetId, AccountSetId, AccountSetId)> {
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let mut ids = Vec::new();
    for name in ["parent", "left", "right"] {
        let set = cala
            .account_sets()
            .create(new_set(AccountSetId::new(), journal.id(), name, None))
            .await?;
        ids.push(set.id());
    }
    Ok((ids[0], ids[1], ids[2]))
}

#[tokio::test]
async fn ensure_member_sets_creates_then_finds_and_only_adds_the_new_edges() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let (parent, left, right) = three_sets(&cala).await?;
    let sets = cala.account_sets();

    let mut op = cala.begin_operation().await?;
    let first = sets
        .ensure_member_sets_in_op(&mut op, &[(parent, left)])
        .await?;
    let again = sets
        .ensure_member_sets_in_op(&mut op, &[(parent, left)])
        .await?;
    let empty = sets.ensure_member_sets_in_op(&mut op, &[]).await?;
    // One edge exists, one is new; the duplicate in the input is ignored.
    let mixed = sets
        .ensure_member_sets_in_op(&mut op, &[(parent, left), (parent, right), (parent, right)])
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    assert!(!again.is_created());
    assert!(!empty.is_created());
    assert!(mixed.is_created());
    let edges: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_account_set_member_account_sets WHERE account_set_id = $1",
    )
    .bind(uuid::Uuid::from(parent))
    .fetch_one(&pool)
    .await?;
    assert_eq!(edges, 2);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_persistent_outbox_events WHERE payload->>'type' = 'account_set_member_created' AND (payload->>'account_set_id')::uuid = $1",
    )
    .bind(uuid::Uuid::from(parent))
    .fetch_one(&pool)
    .await?;
    assert_eq!(events, 2, "an existing edge must not publish again");

    // The non-idempotent variant still rejects an existing edge.
    let res = sets.add_member_sets(&[(parent, left)]).await;
    assert!(matches!(
        res,
        Err(Fail::Rejected(AddSetMembersRejection::MemberAlreadyAdded(
            _
        )))
    ));
    Ok(())
}

#[tokio::test]
async fn ensure_member_sets_concurrent_is_found_not_a_fault() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let (parent, left, _) = three_sets(&cala).await?;

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .account_sets()
        .ensure_member_sets_in_op(&mut op_a, &[(parent, left)])
        .await?;
    let cala_b = cala.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .account_sets()
            .ensure_member_sets_in_op(&mut op_b, &[(parent, left)])
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        op_b.commit().await?;
        anyhow::Ok(ensured)
    });
    let_it_block().await;
    assert!(!b.is_finished(), "B must wait for A's membership lock");
    op_a.commit().await?;
    let b = b.await??;
    assert!(a.is_created());
    assert!(!b.is_created());
    Ok(())
}
