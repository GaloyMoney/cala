mod helpers;

use futures::StreamExt;
use rand::distr::{Alphanumeric, SampleString};
use rust_decimal::Decimal;

use cala_ledger::{
    account::NewAccount,
    calculation::{error::*, *},
    errlanes::Fail,
    outbox::OutboxEventPayload,
    tx_template::Params,
    *,
};

async fn init_test() -> anyhow::Result<(CalaLedger, JournalId)> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    Ok((cala, journal.id()))
}

fn random_code() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

fn new_calculation(journal_id: JournalId, scope: CalculationScope) -> NewCalculation {
    NewCalculation::builder()
        .id(CalculationId::new())
        .journal_id(journal_id)
        .code(random_code())
        .name("Test calculation")
        .scope(scope)
        .build()
        .expect("valid calculation")
}

async fn create_account(cala: &CalaLedger) -> anyhow::Result<AccountId> {
    let code = random_code();
    let account = cala
        .accounts()
        .create(
            NewAccount::builder()
                .id(AccountId::new())
                .name(format!("Account {code}"))
                .code(code)
                .build(),
        )
        .await?;
    Ok(account.id())
}

async fn create_set(cala: &CalaLedger, journal_id: JournalId) -> anyhow::Result<AccountSetId> {
    let set = cala
        .account_sets()
        .create(
            cala_ledger::account_set::NewAccountSet::builder()
                .id(AccountSetId::new())
                .name(format!("Set {}", random_code()))
                .journal_id(journal_id)
                .balance_rollup(BalanceRollup::Synchronous)
                .build(),
        )
        .await?;
    Ok(set.id())
}

#[tokio::test]
async fn create_and_find() -> anyhow::Result<()> {
    let (cala, journal_id) = init_test().await?;

    for (condition, effective_date_source) in [(Some("1 +"), None), (None, Some("date("))] {
        let built = NewCalculation::builder()
            .id(CalculationId::new())
            .journal_id(journal_id)
            .code(random_code())
            .name("Invalid")
            .maybe_condition(condition)
            .maybe_effective_date_source(effective_date_source)
            .build();
        assert!(
            built.is_err(),
            "unparsable expressions must reject at build"
        );
    }
    assert!(NewPartitionKey::builder()
        .alias("d")
        .value("context.vars.entry.(")
        .build()
        .is_err());

    let mut listener = cala.register_outbox_listener(Some(cala.ec_rollup_status().await?.frontier));

    let id = CalculationId::new();
    let new = NewCalculation::builder()
        .id(id)
        .journal_id(journal_id)
        .code(random_code())
        .name("Operating P&L")
        .description("excludes closing")
        .scope(CalculationScope::Global)
        .condition("!has(context.vars.transaction.metadata.is_closing_tx)")
        .dimensions(vec![NewPartitionKey::builder()
            .alias("entry_type")
            .value("context.vars.entry.entry_type")
            .build()?])
        .effective_date_source("context.vars.transaction.effective")
        .build()?;
    let calculation = cala.calculations().create(new).await?;
    let values = calculation.values();
    assert_eq!(values.id, id);
    assert_eq!(values.journal_id, journal_id);
    assert_eq!(values.scope, CalculationScope::Global);
    assert_eq!(values.description.as_deref(), Some("excludes closing"));
    assert!(values.condition.is_some());
    assert_eq!(values.dimensions.len(), 1);
    assert!(values.config.effective_date_source.is_some());

    let found = cala.calculations().find_by_id(id).await?;
    assert_eq!(found.values().code, values.code);
    let by_code = cala.calculations().find_by_code(&values.code).await?;
    assert_eq!(by_code.id(), id);
    let listed = cala.calculations().list_for_journal(journal_id).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id(), id);
    let all = cala
        .calculations()
        .find_all::<Calculation>(&[id, CalculationId::new()])
        .await?;
    assert_eq!(all.len(), 1);

    assert!(matches!(
        cala.calculations().find_by_id(CalculationId::new()).await,
        Err(Fail::Rejected(CalculationNotFound(_)))
    ));
    assert!(matches!(
        cala.calculations().find_by_code("no such code").await,
        Err(Fail::Rejected(CalculationCodeNotFound(_)))
    ));

    let duplicate_id = NewCalculation::builder()
        .id(id)
        .journal_id(journal_id)
        .code(random_code())
        .name("dup id")
        .build()?;
    assert!(matches!(
        cala.calculations().create(duplicate_id).await,
        Err(Fail::Rejected(CreateCalculationRejection::IdAlreadyExists(dup))) if dup == id
    ));

    let duplicate_code = NewCalculation::builder()
        .id(CalculationId::new())
        .journal_id(journal_id)
        .code(values.code.clone())
        .name("dup code")
        .build()?;
    assert!(matches!(
        cala.calculations().create(duplicate_code).await,
        Err(Fail::Rejected(
            CreateCalculationRejection::CodeAlreadyExists(_)
        ))
    ));

    let unknown_journal = JournalId::new();
    assert!(matches!(
        cala.calculations()
            .create(new_calculation(unknown_journal, CalculationScope::Attached))
            .await,
        Err(Fail::Rejected(CreateCalculationRejection::JournalNotFound(j))) if j == unknown_journal
    ));

    let ensured_id = CalculationId::new();
    let ensured_code = random_code();
    let ensure = || {
        NewCalculation::builder()
            .id(ensured_id)
            .journal_id(journal_id)
            .code(ensured_code.clone())
            .name("ensured")
            .build()
            .unwrap()
    };
    let mut op = cala.begin_operation().await?;
    let first = cala.calculations().ensure_in_op(&mut op, ensure()).await?;
    op.commit().await?;
    assert!(first.is_created());
    let mut op = cala.begin_operation().await?;
    let second = cala.calculations().ensure_in_op(&mut op, ensure()).await?;
    op.commit().await?;
    assert!(!second.is_created());

    let created = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = listener.next().await {
            let event = event.expect("outbox delivery");
            if let Some(OutboxEventPayload::CalculationCreated { calculation }) = &event.payload {
                if calculation.id == id {
                    return Some(calculation.clone());
                }
            }
        }
        None
    })
    .await?
    .expect("CalculationCreated must be published");
    assert_eq!(created.code, values.code);

    Ok(())
}

#[derive(Default)]
struct SpanCallCounter {
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanCallCounter {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == "calculation_attachment.create_all_in_op" {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn attach_rules() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;

    let inserts = SpanCallCounter::default();
    let insert_count = inserts.count.clone();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(inserts))
        .expect("no other global tracing subscriber should be set in this test process");

    let (cala, journal_id) = init_test().await?;
    let calculation = cala
        .calculations()
        .create(new_calculation(journal_id, CalculationScope::Attached))
        .await?;
    let id = calculation.id();

    let account = create_account(&cala).await?;
    cala.calculations().attach(id, account).await?;

    let set = create_set(&cala, journal_id).await?;
    cala.calculations().attach(id, set).await?;

    let other_journal = cala.journals().create(helpers::test_journal()).await?.id();
    let foreign_set = create_set(&cala, other_journal).await?;
    assert!(matches!(
        cala.calculations().attach(id, foreign_set).await,
        Err(Fail::Rejected(AttachCalculationRejection::JournalMismatch {
            target,
            target_journal,
            calculation_journal,
        })) if target == foreign_set.into()
            && target_journal == other_journal
            && calculation_journal == journal_id
    ));

    assert!(matches!(
        cala.calculations().attach(id, account).await,
        Err(Fail::Rejected(AttachCalculationRejection::AlreadyAttached))
    ));

    let mut op = cala.begin_operation().await?;
    let ensured = cala
        .calculations()
        .ensure_attached_in_op(&mut op, id, account)
        .await?;
    op.commit().await?;
    assert!(!ensured.is_created());
    let fresh = create_account(&cala).await?;
    let mut op = cala.begin_operation().await?;
    let ensured = cala
        .calculations()
        .ensure_attached_in_op(&mut op, id, fresh)
        .await?;
    op.commit().await?;
    assert!(ensured.is_created());

    let global = cala
        .calculations()
        .create(new_calculation(journal_id, CalculationScope::Global))
        .await?;
    assert!(matches!(
        cala.calculations().attach(global.id(), account).await,
        Err(Fail::Rejected(AttachCalculationRejection::GlobalCannotAttach(g))) if g == global.id()
    ));

    let unknown = AccountId::new();
    assert!(matches!(
        cala.calculations().attach(id, unknown).await,
        Err(Fail::Rejected(AttachCalculationRejection::TargetNotFound(t))) if t == unknown
    ));

    let missing = CalculationId::new();
    assert!(matches!(
        cala.calculations().attach(missing, account).await,
        Err(Fail::Rejected(AttachCalculationRejection::CalculationNotFound(c))) if c == missing
    ));

    let bulk = cala
        .calculations()
        .create(new_calculation(journal_id, CalculationScope::Attached))
        .await?;
    let mut targets = Vec::new();
    for _ in 0..50 {
        targets.push(create_account(&cala).await?);
    }
    let before = insert_count.load(std::sync::atomic::Ordering::SeqCst);
    let mut op = cala.begin_operation().await?;
    cala.calculations()
        .attach_many_in_op(&mut op, bulk.id(), &targets)
        .await?;
    op.commit().await?;
    assert_eq!(
        insert_count.load(std::sync::atomic::Ordering::SeqCst) - before,
        1,
        "50 targets must be attached with one INSERT"
    );

    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = cala
            .calculations()
            .list_attachments(
                bulk.id(),
                es_entity::PaginatedQueryArgs { first: 20, after },
            )
            .await?;
        seen.extend(page.entities().iter().map(|a| a.account_id));
        if !page.has_next_page() {
            break;
        }
        after = page.end_cursor().cloned();
    }
    let mut expected = targets.clone();
    expected.sort();
    assert_eq!(seen, expected);

    Ok(())
}

#[tokio::test]
async fn activation_sequence_is_the_frontier() -> anyhow::Result<()> {
    let (cala, journal_id) = init_test().await?;
    let tx_code = random_code();
    cala.tx_templates()
        .create(helpers::velocity_template(&tx_code))
        .await?;
    let sender = create_account(&cala).await?;
    let recipient = create_account(&cala).await?;
    let post = || {
        let mut params = Params::new();
        params.insert("journal_id", journal_id);
        params.insert("sender", sender);
        params.insert("recipient", recipient);
        params.insert("amount", Decimal::ONE);
        params
    };
    cala.post_transaction(TransactionId::new(), &tx_code, post())
        .await?;
    cala.post_transaction(TransactionId::new(), &tx_code, post())
        .await?;

    let frontier_before = cala.ec_rollup_status().await?.frontier;
    let calculation = cala
        .calculations()
        .create(
            NewCalculation::builder()
                .id(CalculationId::new())
                .journal_id(journal_id)
                .code(random_code())
                .name("Test calculation")
                .scope(CalculationScope::Global)
                .history(History::FromNow)
                .build()?,
        )
        .await?;
    let activation_sequence = calculation.values().activation_sequence;
    assert!(activation_sequence >= u64::from(frontier_before));

    let mut listener = cala.register_outbox_listener(Some(frontier_before));
    let after_id = TransactionId::new();
    cala.post_transaction(after_id, &tx_code, post()).await?;
    let sequence = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = listener.next().await {
            let event = event.expect("outbox delivery");
            if let Some(OutboxEventPayload::TransactionCreated { transaction }) = &event.payload {
                if transaction.id == after_id {
                    return Some(event.sequence);
                }
            }
        }
        None
    })
    .await?
    .expect("TransactionCreated must be published");
    assert!(activation_sequence < u64::from(sequence));

    Ok(())
}
