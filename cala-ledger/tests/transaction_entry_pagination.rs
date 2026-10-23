mod helpers;

use cala_ledger::{tx_template::*, *};

#[tokio::test]
async fn list_for_transaction_id_returns_entries_beyond_the_default_page() -> anyhow::Result<()> {
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
        let listed = cala.entries().list_for_transaction_id(tx.id()).await?;
        println!(
            "posted={count}, stored={}, listed={}",
            stored.len(),
            listed.len()
        );
        assert_eq!(
            listed.len(),
            count,
            "transaction entry listing must not truncate"
        );
        for (index, entry) in listed.iter().enumerate() {
            assert_eq!(entry.values().sequence as usize, index + 1);
            assert!(stored.contains_key(&entry.id()));
        }
    }
    Ok(())
}
