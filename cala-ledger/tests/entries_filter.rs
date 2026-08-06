mod helpers;

use chrono::{Duration, NaiveDate};
use rand::distr::{Alphanumeric, SampleString};

use cala_ledger::{
    entry::{EntriesFilter, Entry, EntryByCreatedAtCursor},
    tx_template::*,
    *,
};

async fn post_tx(
    cala: &CalaLedger,
    code: &str,
    journal_id: JournalId,
    sender: AccountId,
    recipient: AccountId,
    effective: NaiveDate,
) -> TransactionId {
    let mut params = Params::new();
    params.insert("journal_id", journal_id);
    params.insert("sender", sender);
    params.insert("recipient", recipient);
    params.insert("effective", effective);
    let id = TransactionId::new();
    cala.post_transaction(id, code, params).await.unwrap();
    id
}

async fn page(
    cala: &CalaLedger,
    journal_id: JournalId,
    filter: EntriesFilter,
    first: usize,
    after: Option<EntryByCreatedAtCursor>,
) -> es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor> {
    page_dir(
        cala,
        journal_id,
        filter,
        first,
        after,
        es_entity::ListDirection::Descending,
    )
    .await
}

async fn page_dir(
    cala: &CalaLedger,
    journal_id: JournalId,
    filter: EntriesFilter,
    first: usize,
    after: Option<EntryByCreatedAtCursor>,
    direction: es_entity::ListDirection,
) -> es_entity::PaginatedQueryRet<Entry, EntryByCreatedAtCursor> {
    cala.entries()
        .list_for_journal_id_filtered(
            journal_id,
            filter,
            es_entity::PaginatedQueryArgs { first, after },
            direction,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn list_for_journal_id_filtered() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let journal = cala.journals().create(helpers::test_journal()).await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;

    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;

    // Each posting of the currency-conversion template writes 6 entries.
    let jan = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
    let jun = NaiveDate::from_ymd_opt(2020, 6, 20).unwrap();
    let tx_jan = post_tx(&cala, &code, journal.id(), sender.id(), recipient.id(), jan).await;
    let tx_jun = post_tx(&cala, &code, journal.id(), sender.id(), recipient.id(), jun).await;

    // No filter -> every entry in the journal, across both transactions.
    let all = page(&cala, journal.id(), EntriesFilter::default(), 100, None).await;
    assert_eq!(all.entities.len(), 12);
    assert!(!all.has_next_page);

    // effective == jan -> only the January transaction's entries.
    let only_jan = page(
        &cala,
        journal.id(),
        EntriesFilter {
            effective_from: Some(jan),
            effective_to: Some(jan),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert_eq!(only_jan.entities.len(), 6);
    assert!(only_jan
        .entities
        .iter()
        .all(|e| e.values().transaction_id == tx_jan));

    // effective range covering only June.
    let only_jun = page(
        &cala,
        journal.id(),
        EntriesFilter {
            effective_from: Some(NaiveDate::from_ymd_opt(2020, 6, 1).unwrap()),
            effective_to: Some(NaiveDate::from_ymd_opt(2020, 12, 31).unwrap()),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert_eq!(only_jun.entities.len(), 6);
    assert!(only_jun
        .entities
        .iter()
        .all(|e| e.values().transaction_id == tx_jun));

    // effective range that matches nothing.
    let empty = page(
        &cala,
        journal.id(),
        EntriesFilter {
            effective_from: Some(NaiveDate::from_ymd_opt(2019, 1, 1).unwrap()),
            effective_to: Some(NaiveDate::from_ymd_opt(2019, 12, 31).unwrap()),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert!(empty.entities.is_empty());
    assert!(empty.end_cursor.is_none());

    // created_at bounds derived from the entries themselves (avoids clock skew).
    let max_created = all.entities.iter().map(|e| e.created_at()).max().unwrap();
    let up_to_max = page(
        &cala,
        journal.id(),
        EntriesFilter {
            created_at_to: Some(max_created),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert_eq!(up_to_max.entities.len(), 12);

    let after_all = page(
        &cala,
        journal.id(),
        EntriesFilter {
            created_at_from: Some(max_created + Duration::seconds(1)),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert!(after_all.entities.is_empty());

    // created + effective filters compose with AND.
    let combined = page(
        &cala,
        journal.id(),
        EntriesFilter {
            created_at_to: Some(max_created),
            effective_from: Some(jan),
            effective_to: Some(jan),
            ..Default::default()
        },
        100,
        None,
    )
    .await;
    assert_eq!(combined.entities.len(), 6);

    // Cursor pagination composes with a filter: 4 + 2 over the 6 June entries.
    let jun_filter = || EntriesFilter {
        effective_from: Some(jun),
        effective_to: Some(jun),
        ..Default::default()
    };
    let first_page = page(&cala, journal.id(), jun_filter(), 4, None).await;
    assert_eq!(first_page.entities.len(), 4);
    assert!(first_page.has_next_page);

    let second_page = page(&cala, journal.id(), jun_filter(), 4, first_page.end_cursor).await;
    assert_eq!(second_page.entities.len(), 2);
    assert!(!second_page.has_next_page);

    // Ascending direction: oldest first, ordered on (created_at, id) -- entries
    // posted in the same transaction can share a created_at, so the id
    // tie-break matters.
    let asc_first = page_dir(
        &cala,
        journal.id(),
        EntriesFilter::default(),
        5,
        None,
        es_entity::ListDirection::Ascending,
    )
    .await;
    assert_eq!(asc_first.entities.len(), 5);
    assert!(asc_first.has_next_page);
    assert!(asc_first
        .entities
        .windows(2)
        .all(|w| (w[0].created_at(), w[0].id) <= (w[1].created_at(), w[1].id)));

    // Cursor continuation covers the rest with no gaps or overlap.
    let asc_rest = page_dir(
        &cala,
        journal.id(),
        EntriesFilter::default(),
        100,
        asc_first.end_cursor,
        es_entity::ListDirection::Ascending,
    )
    .await;
    assert_eq!(asc_rest.entities.len(), 7);
    assert!(!asc_rest.has_next_page);
    let mut ids: std::collections::HashSet<_> = asc_first.entities.iter().map(|e| e.id).collect();
    for entry in &asc_rest.entities {
        assert!(ids.insert(entry.id));
    }
    assert_eq!(ids.len(), 12);

    // Ascending composes with a filter: June's entries, oldest first.
    let jun_asc = page_dir(
        &cala,
        journal.id(),
        jun_filter(),
        100,
        None,
        es_entity::ListDirection::Ascending,
    )
    .await;
    assert_eq!(jun_asc.entities.len(), 6);
    assert!(jun_asc
        .entities
        .iter()
        .all(|e| e.values().transaction_id == tx_jun));
    assert!(jun_asc
        .entities
        .windows(2)
        .all(|w| (w[0].created_at(), w[0].id) <= (w[1].created_at(), w[1].id)));

    Ok(())
}
