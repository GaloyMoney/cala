mod helpers;

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn list_for_transaction_id_exposes_complete_bidirectional_pagination() -> anyhow::Result<()> {
    let pool = helpers::init_isolated_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(config, &mut jobs).await?;
    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    for count in [2, 100, 102, 250] {
        let mut entries = Vec::new();
        for _ in 0..count / 2 {
            for (account, direction) in [(sender.id(), "'DEBIT'"), (recipient.id(), "'CREDIT'")] {
                entries.push(
                    NewTxTemplateEntry::builder()
                        .entry_type("'PAGINATION_TEST'")
                        .account_id(format!("uuid('{account}')"))
                        .layer("'SETTLED'")
                        .direction(direction)
                        .units("decimal('1')")
                        .currency("'USD'")
                        .build()?,
                );
            }
        }
        let code = format!("pagination_{count}");
        let template = NewTxTemplate::builder()
            .id(TxTemplateId::new())
            .code(&code)
            .transaction(
                NewTxTemplateTransaction::builder()
                    .journal_id(format!("uuid('{}')", journal.id()))
                    .effective("date()")
                    .build()?,
            )
            .entries(entries)
            .build();
        cala.tx_templates().create(template).await?;
        let tx = cala
            .post_transaction(TransactionId::new(), &code, Params::new())
            .await?;
        let stored = cala.entries().find_all(&tx.values().entry_ids).await?;
        assert_eq!(stored.len(), count, "all posted entries must exist");
        for direction in [
            es_entity::ListDirection::Ascending,
            es_entity::ListDirection::Descending,
        ] {
            let mut expected: Vec<_> = stored
                .values()
                .map(|entry| (entry.created_at(), entry.id()))
                .collect();
            expected.sort();
            if matches!(direction, es_entity::ListDirection::Descending) {
                expected.reverse();
            }
            for first in [17, 100] {
                let mut query = es_entity::PaginatedQueryArgs { first, after: None };
                let mut listed = Vec::new();
                loop {
                    let page = cala
                        .entries()
                        .list_for_transaction_id(tx.id(), query, direction)
                        .await?;
                    assert_eq!(page.requested_size(), first);
                    assert!(page.entities().len() <= first);
                    match page.into_page() {
                        es_entity::Page::Last { entities } => {
                            listed.extend(entities);
                            break;
                        }
                        es_entity::Page::HasNext { entities, next } => {
                            assert_eq!(entities.len(), first);
                            assert_eq!(next.first, first);
                            assert_eq!(entities.last().unwrap().id(), next.after.id);
                            listed.extend(entities);
                            query = next.into();
                        }
                    }
                }
                let actual: Vec<_> = listed
                    .iter()
                    .map(|entry| (entry.created_at(), entry.id()))
                    .collect();
                assert_eq!(
                    actual, expected,
                    "pages must preserve order without skips or duplicates"
                );
            }
        }
    }
    let missing = cala
        .entries()
        .list_for_transaction_id(TransactionId::new(), Default::default(), Default::default())
        .await?;
    assert!(
        matches!(missing.into_page(), es_entity::Page::Last { entities } if entities.is_empty())
    );
    Ok(())
}
