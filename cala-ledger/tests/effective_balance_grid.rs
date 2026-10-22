mod helpers;

use cala_ledger::{balance::EffectiveBalanceGrid, tx_template::Params, *};
use chrono::NaiveDate;
use rand::distr::{Alphanumeric, SampleString};

#[tokio::test]
async fn grid_matches_single_ranges_with_backdating_missing_currencies_and_versions(
) -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala = CalaLedger::init(
        CalaLedgerConfig::builder()
            .pool(pool)
            .exec_migrations(false)
            .build()?,
        &mut jobs,
    )
    .await?;
    let journal = cala
        .journals()
        .create(helpers::test_journal_with_effective_balances())
        .await?;
    let (sender, recipient) = helpers::test_accounts();
    let sender = cala.accounts().create(sender).await?;
    let recipient = cala.accounts().create(recipient).await?;
    let code = Alphanumeric.sample_string(&mut rand::rng(), 32);
    cala.tx_templates()
        .create(helpers::currency_conversion_template(&code))
        .await?;
    let date = |value: &str| value.parse::<NaiveDate>().unwrap();
    for effective in ["2024-03-31", "2024-01-31", "2024-03-31", "2024-02-29"] {
        let mut params = Params::new();
        params.insert("journal_id", journal.id());
        params.insert("sender", sender.id());
        params.insert("recipient", recipient.id());
        params.insert("effective", date(effective));
        cala.post_transaction(TransactionId::new(), &code, params)
            .await?;
    }
    let ids: Vec<_> = [sender.id(), recipient.id(), AccountId::new()]
        .into_iter()
        .flat_map(|account| {
            [Currency::USD, Currency::BTC].map(|currency| (journal.id(), account, currency))
        })
        .collect();
    let ranges = [
        (date("2023-12-01"), date("2023-12-31")),
        (date("2024-01-01"), date("2024-01-31")),
        (date("2024-02-01"), date("2024-02-29")),
        (date("2024-03-01"), date("2024-03-31")),
        (date("2024-01-01"), date("2024-03-31")),
        (date("2024-04-01"), date("2024-04-30")),
    ];
    let grid = EffectiveBalanceGrid::try_new(&ids, &ranges)?;
    let mut op = cala.begin_operation().await?;
    let actual = cala
        .balances()
        .effective()
        .find_all_in_ranges_in_op(&mut op, &grid)
        .await?;
    let pooled = cala
        .balances()
        .effective()
        .find_all_in_ranges(&grid)
        .await?;
    for ((from, until), (actual, pooled)) in ranges.iter().zip(actual.iter().zip(&pooled)) {
        let expected = cala
            .balances()
            .effective()
            .find_all_in_range(&ids, *from, Some(*until))
            .await?;
        assert_eq!(actual.len(), expected.len());
        assert_eq!(pooled.len(), expected.len());
        for (id, expected) in expected {
            for actual in [&actual[&id], &pooled[&id]] {
                assert_eq!(actual.open.details, expected.open.details);
                assert_eq!(actual.period.details, expected.period.details);
                assert_eq!(actual.close.details, expected.close.details);
                assert_eq!(actual.period.balance_type, expected.period.balance_type);
            }
        }
    }
    Ok(())
}
