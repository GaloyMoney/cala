use cala_ledger::{account::AccountId, journal::JournalId, CalaLedger};
use cala_perf::{init_accounts, init_cala, init_journal, templates::simple_transfer};
use rand::Rng;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    simple_transfer::init(&cala).await.unwrap();
    let journal = init_journal(&cala).await.unwrap();

    transactions_in_parallel(&cala, journal.id(), 2).await?;

    Ok(())
}

async fn transactions_in_parallel(
    cala: &CalaLedger,
    journal_id: JournalId,
    n: usize,
) -> anyhow::Result<()> {
    // Setup phase: create all pools first
    let mut pools = Vec::new();
    println!("Setting up {} pools...", n);
    
    for i in 0..n {
        let (account1, account2) = init_accounts(cala, false).await?;
        let pool = vec![account1.id(), account2.id()];
        pools.push((i, pool));
        println!("Pool {} created", i);
    }
    
    println!("Setup complete. Starting concurrent execution...");
    
    // Spawn all tasks close together
    let spawn_start = std::time::Instant::now();
    let tasks: Vec<_> = pools.into_iter().map(|(i, pool)| {
        let cala = cala.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            for _ in 0..1000 {
                execute_one_simple_transaction(&cala, journal_id, &pool)
                    .await
                    .unwrap();
            }
            let duration = start.elapsed();
            println!("Pool {}: 1000 transactions completed in {:?}", i, duration);
            (i, duration)
        })
    }).collect();
    
    // Join all tasks and measure total time
    let mut task_results = Vec::new();
    for task in tasks {
        task_results.push(task.await?);
    }
    let total_execution_time = spawn_start.elapsed();
    
    println!("All tasks completed in {:?}", total_execution_time);
    
    for (pool_id, duration) in &task_results {
        println!(
            "Pool {}: Average per transaction: {:?}",
            pool_id,
            *duration / 1000
        );
    }
    
    let total_task_duration: std::time::Duration = task_results.iter().map(|(_, d)| *d).sum();
    let avg_task_duration = total_task_duration / task_results.len() as u32;
    println!("Average pool task duration: {:?}", avg_task_duration);
    println!("Total wall-clock time: {:?}", total_execution_time);
    
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
