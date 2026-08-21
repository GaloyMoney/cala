mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    account::NewAccount,
    account_set::{AccountSetUpdate, NewAccountSet},
    error::LedgerError,
    posting::PostingError,
    velocity::{error::VelocityError, *},
    *,
};

async fn init_test() -> anyhow::Result<(CalaLedger, JournalId, String)> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    Ok((cala, journal.id(), tx_code))
}

async fn control_and_limits(
    velocity: &Velocities,
    limit: Decimal,
) -> anyhow::Result<(VelocityControlId, Params)> {
    let withdrawal_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Withdrawal")
        .description("test")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("params.withdrawal_limit")
                    .enforcement_direction("DEBIT")
                    .always_active()
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![NewParamDefinition::builder()
            .r#type(ParamDataType::Decimal)
            .name("withdrawal_limit")
            .build()
            .expect("param")])
        .build()
        .expect("build limit");

    let withdrawal_limit = velocity.create_limit(withdrawal_limit).await?;

    let deposit_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Deposit")
        .description("test")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("params.deposit_limit")
                    .enforcement_direction("DEBIT")
                    .always_active()
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![NewParamDefinition::builder()
            .r#type(ParamDataType::Decimal)
            .name("deposit_limit")
            .build()
            .expect("param")])
        .build()
        .expect("build limit");
    let deposit_limit = velocity.create_limit(deposit_limit).await?;

    let control = NewVelocityControl::builder()
        .id(VelocityControlId::new())
        .name("test")
        .description("test")
        .build()
        .expect("build control");
    let control = velocity.create_control(control).await?;

    velocity
        .add_limit_to_control(control.id(), withdrawal_limit.id())
        .await?;
    velocity
        .add_limit_to_control(control.id(), deposit_limit.id())
        .await?;

    let mut control_params = Params::new();
    control_params.insert("withdrawal_limit", limit);
    control_params.insert("deposit_limit", limit);

    Ok((control.id(), control_params))
}

async fn account_closing_limit(
    velocity: &Velocities,
    direction: &'static str,
) -> anyhow::Result<VelocityLimit> {
    let new_limit = NewVelocityLimit::builder()
        .id(VelocityLimitId::new())
        .name("Account Closed")
        .description("Ensures no transactions allowed before cutoff date")
        .window(vec![])
        .limit(
            NewLimit::builder()
                .balance(vec![NewBalanceLimit::builder()
                    .layer("SETTLED")
                    .amount("decimal('0')")
                    .enforcement_direction(direction)
                    .always_active()
                    .build()
                    .expect("limit")])
                .build()
                .expect("limit"),
        )
        .params(vec![])
        .build()
        .expect("build limit");

    Ok(velocity.create_limit(new_limit).await?)
}

fn effective_date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn create_control_on_account() -> anyhow::Result<()> {
    let (cala, journal_id, tx_code) = init_test().await?;
    let velocity = cala.velocities();

    let limit = Decimal::ONE_HUNDRED;
    let (control_id, control_params) = control_and_limits(velocity, limit).await?;

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    velocity
        .attach_control_to_account(control_id, sender_account.id(), control_params.clone())
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal_id.to_string());
    tx_params.insert("sender", sender_account.id());
    tx_params.insert("recipient", recipient_account.id());
    tx_params.insert("amount", limit);
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("amount", Decimal::ONE);
    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(res.is_err());

    Ok(())
}

#[tokio::test]
async fn create_control_on_account_set() -> anyhow::Result<()> {
    let (cala, journal_id, tx_code) = init_test().await?;
    let velocity = cala.velocities();

    let limit = Decimal::ONE_HUNDRED;
    let (control_id, control_params) = control_and_limits(velocity, limit).await?;

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    let (new_sender_account_set, _) = helpers::test_account_sets(journal_id.into());
    let sender_account_set = cala
        .account_sets()
        .create(new_sender_account_set)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(sender_account_set.id, sender_account.id)
        .await
        .unwrap();
    velocity
        .attach_control_to_account_set(control_id, sender_account_set.id(), control_params)
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal_id.to_string());
    tx_params.insert("sender", sender_account.id());
    tx_params.insert("recipient", recipient_account.id());
    tx_params.insert("amount", limit);
    let _ = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await?;
    tx_params.insert("amount", Decimal::ONE);
    let res = cala
        .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
        .await;
    assert!(res.is_err());

    Ok(())
}

/// Counts `tracing` spans by exact name — used to prove the batched attach
/// issues one round trip regardless of how many accounts it covers,
/// without depending on `pg_stat_statements` (not test-isolated: shared
/// across concurrent activity on the DB) or a new query-logging mechanism.
/// `tracing-subscriber` is already resolved in this workspace's
/// `Cargo.lock` (pulled in via `tracing-opentelemetry`), so this reuses an
/// already-vetted dependency rather than adding a new one.
///
/// Registered as the process-global default subscriber, not a
/// thread-local one (`tracing::subscriber::with_default`): tokio's
/// multi-thread runtime can resume this test's future on a different
/// worker thread after an `.await`, and a thread-local subscriber would
/// silently miss spans created after such a hop. `cargo-nextest` runs
/// every test in its own OS process, so a process-global subscriber here
/// cannot observe — or be observed by — any other test.
struct SpanCallCounter {
    name: &'static str,
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanCallCounter {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == self.name {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Attaching one control to N accounts must cost a fixed number of round
/// trips, not `3 * N`. This is the regression guard — verified against the
/// specific mechanism (batched `UNNEST` insert) by temporarily reverting
/// `AccountControlRepo::create_all_in_op` to a loop calling `create_in_op`
/// once per account: with that reversion, `per_row_calls` reads 4 instead
/// of 0 and this test fails; restoring the batched insert makes it pass
/// again.
#[tokio::test]
async fn attach_control_to_accounts_in_op_issues_one_batched_insert() -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tracing_subscriber::prelude::*;

    let (cala, _journal_id, _tx_code) = init_test().await?;
    let velocity = cala.velocities();
    let (control_id, control_params) = control_and_limits(velocity, Decimal::ONE_HUNDRED).await?;

    let mut account_ids = Vec::new();
    for _ in 0..4 {
        let (new_account, _) = helpers::test_accounts();
        let account = cala.accounts().create(new_account).await.unwrap();
        account_ids.push(account.id());
    }

    let batched_calls = Arc::new(AtomicUsize::new(0));
    let per_row_calls = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry()
        .with(SpanCallCounter {
            name: "account_control.create_all_in_op",
            count: batched_calls.clone(),
        })
        .with(SpanCallCounter {
            name: "account_control.create_in_op",
            count: per_row_calls.clone(),
        });
    tracing::subscriber::set_global_default(subscriber)
        .expect("no other global tracing subscriber should be set in this test process");

    let mut op = cala.begin_operation().await?;
    velocity
        .attach_control_to_accounts_in_op(&mut op, control_id, &account_ids, control_params)
        .await?;
    op.commit().await?;

    assert_eq!(
        batched_calls.load(Ordering::SeqCst),
        1,
        "attaching to N accounts must issue exactly one batched insert, not N"
    );
    assert_eq!(
        per_row_calls.load(Ordering::SeqCst),
        0,
        "the batched path must never fall back to the per-row insert"
    );

    Ok(())
}

/// Parity with calling the singular API once per account: the limit
/// enforces identically for every account in the batch.
#[tokio::test]
async fn attach_control_to_accounts_in_op_matches_singular_calls() -> anyhow::Result<()> {
    let (cala, journal_id, tx_code) = init_test().await?;
    let velocity = cala.velocities();

    let limit = Decimal::ONE_HUNDRED;
    let (control_id, control_params) = control_and_limits(velocity, limit).await?;

    let (new_recipient, _) = helpers::test_accounts();
    let recipient = cala.accounts().create(new_recipient).await.unwrap();

    let mut senders = Vec::new();
    for _ in 0..3 {
        let (new_sender, _) = helpers::test_accounts();
        senders.push(cala.accounts().create(new_sender).await.unwrap());
    }
    let sender_ids: Vec<_> = senders.iter().map(|s| s.id()).collect();

    let mut op = cala.begin_operation().await?;
    velocity
        .attach_control_to_accounts_in_op(&mut op, control_id, &sender_ids, control_params)
        .await?;
    op.commit().await?;

    for sender in &senders {
        let mut tx_params = Params::new();
        tx_params.insert("journal_id", journal_id.to_string());
        tx_params.insert("sender", sender.id());
        tx_params.insert("recipient", recipient.id());
        tx_params.insert("amount", limit);
        let _ = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await?;
        tx_params.insert("amount", Decimal::ONE);
        let res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(res.is_err(), "limit did not enforce on {:?}", sender.id());
    }

    Ok(())
}

/// A duplicate `account_id` within one batch call hits the same
/// `UNIQUE(account_id, velocity_control_id)` constraint a second singular
/// call would, and aborts the *whole* batch — including the non-duplicate
/// accounts in the same call. Re-attaching the non-duplicate account alone
/// afterwards must still succeed, proving nothing from the failed batch
/// was left attached.
#[tokio::test]
async fn attach_control_to_accounts_in_op_rejects_duplicate_account_id_atomically(
) -> anyhow::Result<()> {
    let (cala, _journal_id, _tx_code) = init_test().await?;
    let velocity = cala.velocities();
    let (control_id, control_params) = control_and_limits(velocity, Decimal::ONE_HUNDRED).await?;

    let (new_account, _) = helpers::test_accounts();
    let account = cala.accounts().create(new_account).await.unwrap();
    let (new_other, _) = helpers::test_accounts();
    let other = cala.accounts().create(new_other).await.unwrap();

    let mut op = cala.begin_operation().await?;
    let res = velocity
        .attach_control_to_accounts_in_op(
            &mut op,
            control_id,
            &[other.id(), account.id(), account.id()],
            control_params.clone(),
        )
        .await;
    assert!(res.is_err());
    drop(op);

    let mut op = cala.begin_operation().await?;
    velocity
        .attach_control_to_accounts_in_op(&mut op, control_id, &[other.id()], control_params)
        .await?;
    op.commit().await?;

    Ok(())
}

/// An empty `account_ids` slice is a no-op insert, not an error.
#[tokio::test]
async fn attach_control_to_accounts_in_op_empty_slice_is_a_noop() -> anyhow::Result<()> {
    let (cala, _journal_id, _tx_code) = init_test().await?;
    let velocity = cala.velocities();
    let (control_id, control_params) = control_and_limits(velocity, Decimal::ONE_HUNDRED).await?;

    let mut op = cala.begin_operation().await?;
    let control = velocity
        .attach_control_to_accounts_in_op(&mut op, control_id, &[], control_params)
        .await?;
    op.commit().await?;

    assert_eq!(control.id(), control_id);

    Ok(())
}

mod limit_via_account_sets {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn create_one_and_limit_with_metadata() -> anyhow::Result<()> {
        let (cala, journal_id, tx_code) = init_test().await?;
        let velocity = cala.velocities();

        let debit_limit = account_closing_limit(velocity, "DEBIT").await?;

        let control = NewVelocityControl::builder()
            .id(VelocityControlId::new())
            .name("Account Closing")
            .description("test")
            .condition("context.vars.account.metadata.closed")
            .build()
            .expect("build control");
        let control = velocity.create_control(control).await?;
        velocity
            .add_limit_to_control(control.id(), debit_limit.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_set = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Account Set {code}"))
            .journal_id(journal_id)
            .metadata(json!({ "closed": true }))
            .unwrap()
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let account_set = cala.account_sets().create(new_account_set).await?;
        velocity
            .attach_control_to_account_set(control.id(), account_set.id(), Params::new())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_open_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Recipient Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let open_account = cala.accounts().create(new_open_account).await.unwrap();

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account = cala.accounts().create(new_account).await.unwrap();
        cala.account_sets()
            .add_member(account_set.id(), account.id())
            .await?;

        let mut tx_params = Params::new();
        tx_params.insert("journal_id", journal_id.to_string());
        tx_params.insert("recipient", open_account.id());
        tx_params.insert("amount", Decimal::ONE);
        tx_params.insert("sender", account.id());
        let account_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        Ok(())
    }

    #[tokio::test]
    async fn create_all_and_limit_with_metadata() -> anyhow::Result<()> {
        let (cala, journal_id, tx_code) = init_test().await?;
        let velocity = cala.velocities();

        let debit_limit = account_closing_limit(velocity, "DEBIT").await?;

        let control = NewVelocityControl::builder()
            .id(VelocityControlId::new())
            .name("Account Closing")
            .description("test")
            .condition("context.vars.account.metadata.closed")
            .build()
            .expect("build control");
        let control = velocity.create_control(control).await?;
        velocity
            .add_limit_to_control(control.id(), debit_limit.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_set_1 = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Account Set {code}"))
            .journal_id(journal_id)
            .metadata(json!({ "closed": true }))
            .unwrap()
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_set_2 = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Account Set {code}"))
            .journal_id(journal_id)
            .metadata(json!({ "closed": true }))
            .unwrap()
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let res = cala
            .account_sets()
            .create_all(vec![new_account_set_1, new_account_set_2])
            .await?;
        let account_set_1 = &res[0];
        velocity
            .attach_control_to_account_set(control.id(), account_set_1.id(), Params::new())
            .await?;
        let account_set_2 = &res[1];
        velocity
            .attach_control_to_account_set(control.id(), account_set_2.id(), Params::new())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_open_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Recipient Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let open_account = cala.accounts().create(new_open_account).await.unwrap();

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_1 = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account_1 = cala.accounts().create(new_account_1).await.unwrap();
        cala.account_sets()
            .add_member(account_set_1.id(), account_1.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_2 = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account_2 = cala.accounts().create(new_account_2).await.unwrap();
        cala.account_sets()
            .add_member(account_set_2.id(), account_2.id())
            .await?;

        let mut tx_params = Params::new();
        tx_params.insert("journal_id", journal_id.to_string());
        tx_params.insert("recipient", open_account.id());
        tx_params.insert("amount", Decimal::ONE);

        tx_params.insert("sender", account_1.id());
        let account_1_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_1_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        tx_params.insert("sender", account_2.id());
        let account_2_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_2_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        Ok(())
    }

    #[tokio::test]
    async fn update_and_limit_with_metadata() -> anyhow::Result<()> {
        let (cala, journal_id, tx_code) = init_test().await?;
        let velocity = cala.velocities();

        let debit_limit = account_closing_limit(velocity, "DEBIT").await?;

        let control = NewVelocityControl::builder()
            .id(VelocityControlId::new())
            .name("Account Closing")
            .description("test")
            .condition(
                r#"
                has(context.vars.account.metadata) &&
                has(context.vars.account.metadata.closed) &&
                context.vars.account.metadata.closed
                "#,
            )
            .build()
            .expect("build control");
        let control = velocity.create_control(control).await?;
        velocity
            .add_limit_to_control(control.id(), debit_limit.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_set = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Account Set {code}"))
            .journal_id(journal_id)
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let mut account_set = cala.account_sets().create(new_account_set).await?;
        velocity
            .attach_control_to_account_set(control.id(), account_set.id(), Params::new())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_open_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Recipient Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let open_account = cala.accounts().create(new_open_account).await.unwrap();

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account = cala.accounts().create(new_account).await.unwrap();
        cala.account_sets()
            .add_member(account_set.id(), account.id())
            .await?;

        let mut tx_params = Params::new();
        tx_params.insert("journal_id", journal_id.to_string());
        tx_params.insert("recipient", open_account.id());
        tx_params.insert("amount", Decimal::ONE);
        tx_params.insert("sender", account.id());

        let account_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        match &account_res {
            Ok(_) => (),
            Err(e) => {
                dbg!(e);
            }
        }
        assert!(account_res.is_ok());

        let mut update = AccountSetUpdate::default();
        update.metadata(json!({ "closed": true }))?;
        if account_set.update(update).did_execute() {
            cala.account_sets().persist(&mut account_set).await?;
        }

        let account_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        Ok(())
    }

    #[tokio::test]
    async fn limit_children_accounts_with_date_via_grandparent_account() -> anyhow::Result<()> {
        let (cala, journal_id, tx_code) = init_test().await?;
        let velocity = cala.velocities();

        let debit_limit = account_closing_limit(velocity, "DEBIT").await?;

        let control = NewVelocityControl::builder()
            .id(VelocityControlId::new())
            .name("Account Closing")
            .description("test")
            .condition(
                r#"
                !has(context.vars.account.metadata) ||
                !has(context.vars.account.metadata.closedAsOf) ||
                date(context.vars.account.metadata.closedAsOf) >= context.vars.transaction.effective
                "#,
            )
            .build()
            .expect("build control");

        let control = velocity.create_control(control).await?;
        velocity
            .add_limit_to_control(control.id(), debit_limit.id())
            .await?;

        // Setup account sets and accounts in dag
        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_parent_account_set = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Parent Account Set {code}"))
            .journal_id(journal_id)
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let mut parent_account_set = cala
            .account_sets()
            .create(new_parent_account_set)
            .await
            .unwrap();
        velocity
            .attach_control_to_account_set(control.id(), parent_account_set.id(), Params::new())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_child_account_set = NewAccountSet::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Child Account Set {code}"))
            .journal_id(journal_id)
            .balance_rollup(BalanceRollup::Synchronous)
            .build()
            .unwrap();
        let child_account_set = cala
            .account_sets()
            .create(new_child_account_set)
            .await
            .unwrap();
        cala.account_sets()
            .add_member(parent_account_set.id(), child_account_set.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_1 = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account_1 = cala.accounts().create(new_account_1).await.unwrap();
        cala.account_sets()
            .add_member(child_account_set.id(), account_1.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_account_2 = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Sender Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let account_2 = cala.accounts().create(new_account_2).await.unwrap();
        cala.account_sets()
            .add_member(child_account_set.id(), account_2.id())
            .await?;

        let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
        let new_open_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .name(format!("Test Recipient Account {code}"))
            .code(code)
            .build()
            .unwrap();
        let open_account = cala.accounts().create(new_open_account).await.unwrap();

        // Execute transactions
        let mut tx_params = Params::new();
        tx_params.insert("journal_id", journal_id.to_string());
        tx_params.insert("recipient", open_account.id());
        tx_params.insert("amount", Decimal::ONE);

        tx_params.insert("effective", effective_date(2025, 1, 1));

        tx_params.insert("sender", account_1.id());
        let account_1_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_1_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        tx_params.insert("sender", account_2.id());
        let account_2_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_2_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        // Add first closing date and re-check
        let mut update = AccountSetUpdate::default();
        update.metadata(json!({ "closedAsOf": "2024-12-31" }))?;
        if parent_account_set.update(update).did_execute() {
            cala.account_sets().persist(&mut parent_account_set).await?;
        }

        tx_params.insert("sender", account_1.id());
        let account_1_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(account_1_send_res.is_ok());

        tx_params.insert("sender", account_2.id());
        let account_2_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(account_2_send_res.is_ok());

        // Check before closing date
        tx_params.insert("effective", effective_date(2024, 12, 31));

        tx_params.insert("sender", account_1.id());
        let account_1_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_1_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        tx_params.insert("sender", account_2.id());
        let account_2_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_2_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        // Update closing date and re-check
        let mut update = AccountSetUpdate::default();
        update.metadata(json!({ "closedAsOf": "2025-01-31" }))?;
        if parent_account_set.update(update).did_execute() {
            cala.account_sets().persist(&mut parent_account_set).await?;
        }

        tx_params.insert("effective", effective_date(2025, 1, 1));

        tx_params.insert("sender", account_1.id());
        let account_1_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_1_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        tx_params.insert("sender", account_2.id());
        let account_2_send_res = cala
            .post_transaction(TransactionId::new(), &tx_code, tx_params.clone())
            .await;
        assert!(matches!(
            account_2_send_res,
            Err(LedgerError::PostingError(PostingError::VelocityError(
                VelocityError::Enforcement(_)
            )))
        ));

        Ok(())
    }
}
