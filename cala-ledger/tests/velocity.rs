mod helpers;

use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{velocity::*, *};

#[tokio::test]
async fn create_control_on_account() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config).await?;

    let new_journal = helpers::test_journal();
    let journal = cala.journals().create(new_journal).await.unwrap();

    let velocity = cala.velocities();

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

    let tx_code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let new_template = helpers::velocity_template(&tx_code);
    cala.tx_templates().create(new_template).await.unwrap();

    let mut control_params = Params::new();
    let limit = Decimal::ONE_HUNDRED;
    control_params.insert("withdrawal_limit", limit);
    control_params.insert("deposit_limit", limit);

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    velocity
        .attach_control_to_account(control.id(), sender_account.id(), control_params.clone())
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal.id().to_string());
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

    let (sender, receiver) = helpers::test_accounts();
    let sender_account = cala.accounts().create(sender).await.unwrap();
    let recipient_account = cala.accounts().create(receiver).await.unwrap();
    let (new_sender_account_set, new_recipient_account_set) =
        helpers::test_account_sets(journal.id().into());
    let sender_account_set = cala
        .account_sets()
        .create(new_sender_account_set)
        .await
        .unwrap();
    let recipient_account_set = cala
        .account_sets()
        .create(new_recipient_account_set)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(sender_account_set.id, sender_account.id)
        .await
        .unwrap();
    cala.account_sets()
        .add_member(recipient_account_set.id, recipient_account.id)
        .await
        .unwrap();
    velocity
        .attach_control_to_account(control.id(), sender_account_set.id(), control_params)
        .await?;

    let mut tx_params = Params::new();
    tx_params.insert("journal_id", journal.id().to_string());
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
