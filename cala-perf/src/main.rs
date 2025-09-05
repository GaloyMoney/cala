use cala_ledger::{account::AccountId, journal::JournalId, CalaLedger};
use cala_perf::{init_accounts, init_cala, init_journal, templates::simple_transfer};
use rand::Rng;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    simple_transfer::init(&cala).await.unwrap();
    let journal = init_journal(&cala).await.unwrap();

    let (a, b) = init_accounts(&cala, false).await?;
    let (c, d) = init_accounts(&cala, false).await?;

    let pool_a = vec![a.id(), b.id()];
    let pool_b = vec![c.id(), d.id()];

    transactions_in_parallel(&cala, journal.id(), &pool_a, &pool_b).await?;

    Ok(())
}

async fn transactions_in_parallel(
    cala: &CalaLedger,
    journal_id: JournalId,
    pool_a: &[AccountId],
    pool_b: &[AccountId],
) -> anyhow::Result<()> {
    let task_a = {
        let cala = cala.clone();
        let pool = pool_a.to_vec();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            for _ in 0..1000 {
                execute_one_simple_transaction(&cala, journal_id, &pool)
                    .await
                    .unwrap();
            }
            let duration = start.elapsed();
            println!("Pool A: 1000 transactions completed in {:?}", duration);
            duration
        })
    };

    let task_b = {
        let cala = cala.clone();
        let pool = pool_b.to_vec();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            for _ in 0..1000 {
                execute_one_simple_transaction(&cala, journal_id, &pool)
                    .await
                    .unwrap();
            }
            let duration = start.elapsed();
            println!("Pool B: 1000 transactions completed in {:?}", duration);
            duration
        })
    };

    let (duration_a, duration_b) = tokio::try_join!(task_a, task_b)?;

    println!(
        "Total execution time - Pool A: {:?}, Pool B: {:?}",
        duration_a, duration_b
    );
    println!(
        "Average per transaction - Pool A: {:?}, Pool B: {:?}",
        duration_a / 1000,
        duration_b / 1000
    );

    Ok(())
}

async fn execute_one_simple_transaction(
    cala: &CalaLedger,
    journal_id: JournalId,
    pool: &[AccountId],
) -> anyhow::Result<()> {
    let (sender, recipient) = pick_two_random_accounts(pool);
    simple_transfer::execute(cala, journal_id, sender, recipient).await
}

fn pick_two_random_accounts(accounts: &[AccountId]) -> (AccountId, AccountId) {
    if accounts.len() < 2 {
        unreachable!();
    }

    let mut rng = rand::rng();
    let first_idx = rng.random_range(0..accounts.len());
    let mut second_idx = rng.random_range(0..accounts.len());
    while second_idx == first_idx {
        second_idx = rng.random_range(0..accounts.len());
    }
    (accounts[first_idx], accounts[second_idx])
}
