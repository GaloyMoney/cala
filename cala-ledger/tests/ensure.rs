//! `ensure_*_in_op`: create-or-find on tx templates, journals, velocity limits
//! and controls, accounts and account sets, plus the idempotent attach / add-member
//! variants. Each `Found` returns the stored entity and persists nothing.
mod helpers;

use cala_ledger::{
    account::{error::*, NewAccount},
    account_set::{error::*, NewAccountSet},
    errlanes::Fail,
    journal::{error::*, NewJournal},
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
            EnsureTxTemplateRejection::KeysSpanEntities { .. } => {}
        }
    }
    fn journal(r: EnsureJournalRejection) {
        match r {
            EnsureJournalRejection::KeysSpanEntities { .. } => {}
        }
    }
    fn account(r: EnsureAccountRejection) {
        match r {
            EnsureAccountRejection::InitialAccountSetNotFound(_)
            | EnsureAccountRejection::KeysSpanEntities { .. } => {}
        }
    }
    fn account_set(r: EnsureAccountSetRejection) {
        match r {
            EnsureAccountSetRejection::KeysSpanEntities { .. }
            | EnsureAccountSetRejection::BackingDuplicateId(_)
            | EnsureAccountSetRejection::BackingCodeAlreadyExists(_) => {}
        }
    }
    let _ = (tx_template, journal, account, account_set);

    assert_eq!(
        codes::<EnsureTxTemplateRejectionCode>(),
        ["CALA_TX_TEMPLATE_KEYS_SPAN_ENTITIES"]
    );
    assert_eq!(
        codes::<EnsureJournalRejectionCode>(),
        ["CALA_JOURNAL_KEYS_SPAN_ENTITIES"]
    );
    assert_eq!(
        codes::<EnsureAccountRejectionCode>(),
        [
            "CALA_ACCOUNT_INITIAL_ACCOUNT_SET_NOT_FOUND",
            "CALA_ACCOUNT_KEYS_SPAN_ENTITIES"
        ]
    );
    assert_eq!(
        codes::<EnsureAccountSetRejectionCode>(),
        [
            "CALA_ACCOUNT_SET_KEYS_SPAN_ENTITIES",
            "CALA_ACCOUNT_SET_BACKING_ACCOUNT_ID_ALREADY_EXISTS",
            "CALA_ACCOUNT_SET_BACKING_ACCOUNT_CODE_ALREADY_EXISTS"
        ]
    );
}

fn sorted<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    let mut items = items.to_vec();
    items.sort();
    items
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
async fn tx_template_ensure_found_through_code_returns_stored_not_new() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let code = random("ensure-tpl-stored");

    let mut op = cala.begin_operation().await?;
    let stored = cala
        .tx_templates()
        .ensure_in_op(&mut op, helpers::currency_conversion_template(&code))
        .await?
        .into_inner();
    op.commit().await?;

    // Same code, different body and a fresh id: found through the code.
    let fresh_id = TxTemplateId::new();
    let mut other = template_with_id(fresh_id, &code);
    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala.tx_templates().ensure_in_op(&mut op, other).await? else {
        panic!("a taken code must be found");
    };
    op.commit().await?;
    assert_eq!(found.id(), stored.id());
    let fresh_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cala_tx_templates WHERE id = $1")
            .bind(uuid::Uuid::from(fresh_id))
            .fetch_one(&pool)
            .await?;
    assert_eq!(fresh_rows, 0, "Found must not write the new id");

    other = helpers::simple_template_with_date_default(&code);
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
async fn tx_template_ensure_found_through_id_keeps_the_stored_code() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let reused_id = TxTemplateId::new();
    let code = random("ensure-tpl-id-a");
    cala.tx_templates()
        .create(template_with_id(reused_id, &code))
        .await?;

    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala
        .tx_templates()
        .ensure_in_op(
            &mut op,
            template_with_id(reused_id, &random("ensure-tpl-id-b")),
        )
        .await?
    else {
        panic!("a taken id must be found");
    };
    assert_eq!(found.id(), reused_id);
    assert_eq!(found.values().code, code);
    Ok(())
}

#[tokio::test]
async fn tx_template_ensure_keys_spanning_two_templates_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let x = cala
        .tx_templates()
        .create(template_with_id(
            TxTemplateId::new(),
            &random("ensure-tpl-x"),
        ))
        .await?;
    let y_code = random("ensure-tpl-y");
    let y = cala
        .tx_templates()
        .create(template_with_id(TxTemplateId::new(), &y_code))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .tx_templates()
        .ensure_in_op(&mut op, template_with_id(x.id(), &y_code))
        .await
        .err()
        .expect("keys held by two templates must be rejected");
    let Fail::Rejected(EnsureTxTemplateRejection::KeysSpanEntities { ids, keys }) = err else {
        panic!("expected KeysSpanEntities, got {err:?}");
    };
    assert_eq!(sorted(&ids), sorted(&[x.id(), y.id()]));
    assert_eq!(sorted(&keys), ["code", "id"]);
    // The rejection did not abort the transaction.
    cala.tx_templates()
        .create_in_op(
            &mut op,
            template_with_id(TxTemplateId::new(), &random("ensure-tpl-after")),
        )
        .await?;
    op.commit().await?;
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

// --- journals -------------------------------------------------------------

fn new_journal(id: JournalId, code: Option<&str>, name: &str) -> NewJournal {
    let mut builder = NewJournal::builder();
    builder.id(id).name(name);
    if let Some(code) = code {
        builder.code(code);
    }
    builder.build().unwrap()
}

#[tokio::test]
async fn journal_ensure_creates_then_finds() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let id = JournalId::new();
    let code = random("ensure-journal");

    let mut op = cala.begin_operation().await?;
    let first = cala
        .journals()
        .ensure_in_op(&mut op, new_journal(id, Some(&code), "first"))
        .await?;
    let second = cala
        .journals()
        .ensure_in_op(&mut op, new_journal(id, Some(&code), "second"))
        .await?;
    op.commit().await?;

    assert!(first.is_created());
    let Ensured::Found(found) = second else {
        panic!("the same journal must be found");
    };
    assert_eq!(found.id(), id);
    assert_eq!(found.values().name, "first");
    Ok(())
}

#[tokio::test]
async fn journal_ensure_found_through_code_returns_stored_not_new() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let code = random("ensure-journal-code");
    let stored = cala
        .journals()
        .create(new_journal(JournalId::new(), Some(&code), "stored"))
        .await?;

    let fresh_id = JournalId::new();
    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala
        .journals()
        .ensure_in_op(&mut op, new_journal(fresh_id, Some(&code), "other"))
        .await?
    else {
        panic!("a taken code must be found");
    };
    op.commit().await?;

    assert_eq!(found.id(), stored.id());
    assert_eq!(found.values().name, "stored");
    let fresh_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cala_journals WHERE id = $1")
        .bind(uuid::Uuid::from(fresh_id))
        .fetch_one(&pool)
        .await?;
    assert_eq!(fresh_rows, 0, "Found must not write the new id");
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cala_journal_events e JOIN cala_journals j ON j.id = e.id WHERE j.code = $1",
    )
    .bind(&code)
    .fetch_one(&pool)
    .await?;
    assert_eq!(events, 1, "Found must not persist the new events");
    Ok(())
}

#[tokio::test]
async fn journal_ensure_found_through_id_keeps_the_stored_code() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let reused_id = JournalId::new();
    let code = random("ensure-journal-id-a");
    cala.journals()
        .create(new_journal(reused_id, Some(&code), "stored"))
        .await?;

    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala
        .journals()
        .ensure_in_op(
            &mut op,
            new_journal(reused_id, Some(&random("ensure-journal-id-b")), "other"),
        )
        .await?
    else {
        panic!("a taken id must be found");
    };
    assert_eq!(found.id(), reused_id);
    assert_eq!(found.values().code.as_deref(), Some(code.as_str()));
    Ok(())
}

#[tokio::test]
async fn journal_ensure_absent_code_does_not_match() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let mut op = cala.begin_operation().await?;
    for name in ["a", "b"] {
        let ensured = cala
            .journals()
            .ensure_in_op(&mut op, new_journal(JournalId::new(), None, name))
            .await?;
        assert!(ensured.is_created(), "two NULL codes are no match");
    }
    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn journal_ensure_keys_spanning_two_journals_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let x = cala
        .journals()
        .create(new_journal(
            JournalId::new(),
            Some(&random("ensure-journal-x")),
            "x",
        ))
        .await?;
    let y_code = random("ensure-journal-y");
    let y = cala
        .journals()
        .create(new_journal(JournalId::new(), Some(&y_code), "y"))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .journals()
        .ensure_in_op(&mut op, new_journal(x.id(), Some(&y_code), "z"))
        .await
        .err()
        .expect("keys held by two journals must be rejected");
    let Fail::Rejected(EnsureJournalRejection::KeysSpanEntities { ids, keys }) = err else {
        panic!("expected KeysSpanEntities, got {err:?}");
    };
    assert_eq!(sorted(&ids), sorted(&[x.id(), y.id()]));
    assert_eq!(sorted(&keys), ["code", "id"]);
    // The rejection did not abort the transaction.
    cala.journals()
        .create_in_op(&mut op, new_journal(JournalId::new(), None, "after"))
        .await?;
    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn journal_ensure_concurrent_creates_once_and_finds_once() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let code = random("ensure-journal-concurrent");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .journals()
        .ensure_in_op(&mut op_a, new_journal(JournalId::new(), Some(&code), "a"))
        .await?;

    let cala_b = cala.clone();
    let code_b = code.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let ensured = cala_b
            .journals()
            .ensure_in_op(&mut op_b, new_journal(JournalId::new(), Some(&code_b), "b"))
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
        panic!("the loser must find the winner's journal");
    };
    assert_eq!(found.id(), a.into_inner().id());
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cala_journals WHERE code = $1")
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
async fn account_ensure_found_through_code_or_external_id_returns_stored_id() -> anyhow::Result<()>
{
    let (cala, pool) = init().await?;
    let code = random("ensure-acc-code");
    let external_id = random("ensure-acc-ext");
    let stored_id = AccountId::new();

    let mut op = cala.begin_operation().await?;
    let first = cala
        .accounts()
        .ensure_in_op(
            &mut op,
            new_account(stored_id, &code, Some(&external_id), "first"),
        )
        .await?;
    assert!(first.is_created());

    let by_code_id = AccountId::new();
    let Ensured::Found(by_code) = cala
        .accounts()
        .ensure_in_op(&mut op, new_account(by_code_id, &code, None, "by code"))
        .await?
    else {
        panic!("a taken code must be found");
    };
    let by_external_id_id = AccountId::new();
    let Ensured::Found(by_external_id) = cala
        .accounts()
        .ensure_in_op(
            &mut op,
            new_account(
                by_external_id_id,
                &random("c"),
                Some(&external_id),
                "by ext",
            ),
        )
        .await?
    else {
        panic!("a taken external id must be found");
    };
    op.commit().await?;

    assert_eq!(by_code.id(), stored_id);
    assert_eq!(by_external_id.id(), stored_id);
    assert_eq!(by_external_id.values().name, "first");
    let fresh_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cala_accounts WHERE id = ANY($1)")
            .bind(vec![
                uuid::Uuid::from(by_code_id),
                uuid::Uuid::from(by_external_id_id),
            ])
            .fetch_one(&pool)
            .await?;
    assert_eq!(fresh_rows, 0, "Found must not write the new ids");
    Ok(())
}

#[tokio::test]
async fn account_ensure_absent_external_id_does_not_match() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let mut op = cala.begin_operation().await?;
    for name in ["a", "b"] {
        let ensured = cala
            .accounts()
            .ensure_in_op(
                &mut op,
                new_account(AccountId::new(), &random("c"), None, name),
            )
            .await?;
        assert!(ensured.is_created(), "two NULL external ids are no match");
    }
    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn account_ensure_keys_spanning_two_accounts_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let x = cala
        .accounts()
        .create(new_account(AccountId::new(), &random("c"), None, "x"))
        .await?;
    let y_code = random("c");
    let y_external_id = random("e");
    let y = cala
        .accounts()
        .create(new_account(
            AccountId::new(),
            &y_code,
            Some(&y_external_id),
            "y",
        ))
        .await?;

    let mut op = cala.begin_operation().await?;
    // Id held by X, code by Y.
    let err = cala
        .accounts()
        .ensure_in_op(&mut op, new_account(x.id(), &y_code, None, "both"))
        .await
        .err()
        .expect("keys held by two accounts must be rejected");
    let Fail::Rejected(EnsureAccountRejection::KeysSpanEntities { ids, keys }) = err else {
        panic!("expected KeysSpanEntities, got {err:?}");
    };
    assert_eq!(sorted(&ids), sorted(&[x.id(), y.id()]));
    assert!(keys.contains(&"id") && keys.contains(&"code"), "{keys:?}");
    assert!(!keys.contains(&"external_id"), "{keys:?}");

    // Code held by X, external id by Y.
    let err = cala
        .accounts()
        .ensure_in_op(
            &mut op,
            new_account(
                AccountId::new(),
                &x.values().code,
                Some(&y_external_id),
                "both",
            ),
        )
        .await
        .err()
        .expect("keys held by two accounts must be rejected");
    let Fail::Rejected(EnsureAccountRejection::KeysSpanEntities { ids, keys }) = err else {
        panic!("expected KeysSpanEntities, got {err:?}");
    };
    assert_eq!(sorted(&ids), sorted(&[x.id(), y.id()]));
    assert_eq!(sorted(&keys), ["code", "external_id"]);

    // The rejections did not abort the transaction.
    cala.accounts()
        .create_in_op(
            &mut op,
            new_account(AccountId::new(), &random("c"), None, "after"),
        )
        .await?;
    op.commit().await?;
    Ok(())
}

#[tokio::test]
async fn account_ensure_concurrent_creates_once_and_finds_once() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let external_id = random("ensure-acc-conc");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .accounts()
        .ensure_in_op(
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
            .ensure_in_op(
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
        .ensure_in_op(
            &mut op,
            new_set(first_id, journal.id(), "first", Some(&external_id)),
        )
        .await?;
    op.commit().await?;
    assert!(first.is_created());

    let mut op = cala.begin_operation().await?;
    let second = cala
        .account_sets()
        .ensure_in_op(
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
async fn account_set_ensure_found_through_id_keeps_the_stored_set() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let existing = cala
        .account_sets()
        .create(new_set(AccountSetId::new(), journal.id(), "holder", None))
        .await?;

    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala
        .account_sets()
        .ensure_in_op(
            &mut op,
            new_set(existing.id(), journal.id(), "same id", Some(&random("e"))),
        )
        .await?
    else {
        panic!("a taken set id must be found");
    };
    op.commit().await?;
    assert_eq!(found.id(), existing.id());
    assert_eq!(found.values().name, "holder");
    assert_eq!(found.values().external_id, None);
    assert_eq!(orphan_backing_accounts(&pool).await?, 0);
    Ok(())
}

#[tokio::test]
async fn account_set_ensure_id_held_by_a_plain_account_is_a_rejection() -> anyhow::Result<()> {
    let (cala, _) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let plain = cala
        .accounts()
        .create(new_account(AccountId::new(), &random("c"), None, "plain"))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .account_sets()
        .ensure_in_op(
            &mut op,
            new_set(
                AccountSetId::from(uuid::Uuid::from(plain.id())),
                journal.id(),
                "same id",
                Some(&random("e")),
            ),
        )
        .await
        .err()
        .expect("a backing id held by a plain account must be rejected");
    let Fail::Rejected(EnsureAccountSetRejection::BackingDuplicateId(id)) = err else {
        panic!("expected BackingDuplicateId, got {err:?}");
    };
    assert_eq!(id, plain.id());
    Ok(())
}

#[tokio::test]
async fn account_set_ensure_keys_spanning_two_sets_is_a_rejection() -> anyhow::Result<()> {
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let x = cala
        .account_sets()
        .create(new_set(AccountSetId::new(), journal.id(), "x", None))
        .await?;
    let y_external_id = random("ensure-set-y");
    let y = cala
        .account_sets()
        .create(new_set(
            AccountSetId::new(),
            journal.id(),
            "y",
            Some(&y_external_id),
        ))
        .await?;

    let mut op = cala.begin_operation().await?;
    let err = cala
        .account_sets()
        .ensure_in_op(
            &mut op,
            new_set(x.id(), journal.id(), "both", Some(&y_external_id)),
        )
        .await
        .err()
        .expect("keys held by two sets must be rejected");
    let Fail::Rejected(EnsureAccountSetRejection::KeysSpanEntities { ids, keys }) = err else {
        panic!("expected KeysSpanEntities, got {err:?}");
    };
    assert_eq!(sorted(&ids), sorted(&[x.id(), y.id()]));
    assert_eq!(sorted(&keys), ["external_id", "id"]);
    // The rejection did not abort the transaction and wrote nothing.
    cala.account_sets()
        .create_in_op(
            &mut op,
            new_set(AccountSetId::new(), journal.id(), "after", None),
        )
        .await?;
    op.commit().await?;
    assert_eq!(orphan_backing_accounts(&pool).await?, 0);
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
        .ensure_in_op(
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
            .ensure_in_op(
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

/// A plain `create_in_op` of the same set id commits while the ensure waits
/// on its backing insert: that is a retryable race, not `BackingDuplicateId`,
/// and the retry finds the set.
#[tokio::test]
async fn account_set_ensure_racing_a_plain_create_of_the_same_id_is_transient() -> anyhow::Result<()>
{
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let journal_id = journal.id();
    let set_id = AccountSetId::new();

    let mut op_a = cala.begin_operation().await?;
    cala.account_sets()
        .create_in_op(&mut op_a, new_set(set_id, journal_id, "a", None))
        .await?;
    let cala_b = cala.clone();
    let b = tokio::spawn(async move {
        let mut op_b = cala_b.begin_operation().await?;
        let res = cala_b
            .account_sets()
            .ensure_in_op(&mut op_b, new_set(set_id, journal_id, "b", None))
            .await;
        anyhow::Ok(res.map(|ensured| ensured.is_created()))
    });
    let_it_block().await;
    assert!(!b.is_finished(), "B must wait for A's backing account");
    op_a.commit().await?;
    let res = b.await??;
    assert!(
        matches!(res, Err(Fail::Transient(_))),
        "expected a transient conflict, got {res:?}"
    );

    let mut op = cala.begin_operation().await?;
    let Ensured::Found(found) = cala
        .account_sets()
        .ensure_in_op(&mut op, new_set(set_id, journal_id, "retry", None))
        .await?
    else {
        panic!("the retry must find A's set");
    };
    op.commit().await?;
    assert_eq!(found.values().name, "a");
    assert_eq!(orphan_backing_accounts(&pool).await?, 0);
    Ok(())
}

/// A holds id `s` and external id `e`, uncommitted. B shares only the id, C
/// only the external id: each waits on the per-key lock it shares with A, then
/// finds A's set through that key. Nobody deadlocks or orphans a backing
/// account.
#[tokio::test]
async fn account_set_ensure_concurrent_on_either_key_finds_through_that_key() -> anyhow::Result<()>
{
    let (cala, pool) = init().await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let journal_id = journal.id();
    let set_id = AccountSetId::new();
    let external_id = random("ensure-set-keys");

    let mut op_a = cala.begin_operation().await?;
    let a = cala
        .account_sets()
        .ensure_in_op(
            &mut op_a,
            new_set(set_id, journal_id, "a", Some(&external_id)),
        )
        .await?;
    let spawn_ensure = |new: NewAccountSet| {
        let cala = cala.clone();
        tokio::spawn(async move {
            let mut op = cala.begin_operation().await?;
            let ensured = cala
                .account_sets()
                .ensure_in_op(&mut op, new)
                .await
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            op.commit().await?;
            anyhow::Ok(ensured)
        })
    };
    let b = spawn_ensure(new_set(
        set_id,
        journal_id,
        "b",
        Some(&random("ensure-set-other")),
    ));
    let c = spawn_ensure(new_set(
        AccountSetId::new(),
        journal_id,
        "c",
        Some(&external_id),
    ));
    let_it_block().await;
    assert!(!b.is_finished(), "B must wait for A on the id");
    assert!(!c.is_finished(), "C must wait for A on the external id");
    op_a.commit().await?;
    let (b, c) = (b.await??, c.await??);

    assert!(a.is_created());
    for (name, ensured) in [("B", b), ("C", c)] {
        let Ensured::Found(found) = ensured else {
            panic!("{name} must find A's set");
        };
        assert_eq!(found.id(), set_id);
        assert_eq!(found.values().name, "a");
    }
    assert_eq!(orphan_backing_accounts(&pool).await?, 0);
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
