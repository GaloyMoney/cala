use cala_ledger::{account::AccountId, account_set::*, journal::JournalId, CalaLedger};
use cala_perf::{init_accounts, init_cala, init_journal, templates::simple_transfer};
use rand::Rng;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cala = init_cala().await?;
    simple_transfer::init(&cala).await.unwrap();
    let journal = init_journal(&cala, false).await.unwrap();

    println!("🚀 Starting parallel transaction performance tests\n");
    println!(
        "🏊 Tokio worker threads: {}",
        tokio::runtime::Handle::current().metrics().num_workers()
    );

    let mut results = Vec::new();

    for &pool_count in &[1, 2, 5] {
        println!("{}", "=".repeat(80));
        println!("🏊 Testing with {} parallel pools", pool_count);
        println!("{}", "=".repeat(80));

        let tx_per_sec = transactions_in_parallel(&cala, journal.id(), pool_count).await?;
        results.push((format!("{} parallel", pool_count), tx_per_sec));

        println!("\n");
    }

    // Test contention scenarios
    for &parallel_count in &[2, 5] {
        println!("{}", "=".repeat(80));
        println!(
            "⚔️  Testing with contention - {} parallel tasks on shared pool",
            parallel_count
        );
        println!("{}", "=".repeat(80));

        let tx_per_sec =
            transactions_with_contention(&cala, journal.id(), 3, parallel_count, None).await?;
        results.push((format!("{} contention", parallel_count), tx_per_sec));

        println!("\n");
    }

    // Test account sets scenarios
    for &parallel_count in &[2, 5] {
        println!("{}", "=".repeat(80));
        println!(
            "🗂️  Testing with account sets - {} parallel tasks, 1 account set",
            parallel_count
        );
        println!("{}", "=".repeat(80));

        let tx_per_sec =
            transactions_with_contention(&cala, journal.id(), 4, parallel_count, Some(1)).await?;
        results.push((format!("{} acct_sets", parallel_count), tx_per_sec));

        println!("\n");
    }

    // Final summary table
    println!("{}", "=".repeat(80));
    println!("📋 PERFORMANCE SUMMARY TABLE");
    println!("{}", "=".repeat(80));
    println!();
    println!("| Scenario        | tx/s     |");
    println!("|-----------------|----------|");
    for (scenario, tx_per_sec) in &results {
        println!("| {:<15} | {:<8.2} |", scenario, tx_per_sec);
    }
    println!();
    println!("✅ All performance tests completed!");

    Ok(())
}

async fn transactions_in_parallel(
    cala: &CalaLedger,
    journal_id: JournalId,
    n: usize,
) -> anyhow::Result<f64> {
    // Setup phase: create all pools first
    let mut pools = Vec::new();
    println!("🔧 Setting up {} pools...", n);

    for i in 0..n {
        let (account1, account2) = init_accounts(cala, false).await?;
        let pool = vec![account1.id(), account2.id()];
        pools.push((i, pool));
        if n <= 10 || i % 10 == 9 || i == n - 1 {
            println!("  ✓ Pool {} created", i);
        }
    }

    println!("🎯 Setup complete. Starting concurrent execution...");

    // Spawn all tasks close together
    let spawn_start = std::time::Instant::now();
    let tasks: Vec<_> = pools
        .into_iter()
        .map(|(i, pool)| {
            let cala = cala.clone();
            tokio::spawn(async move {
                let start = std::time::Instant::now();
                for _ in 0..1000 {
                    execute_one_simple_transaction(&cala, journal_id, &pool)
                        .await
                        .unwrap();
                }
                let duration = start.elapsed();
                if n <= 10 {
                    println!(
                        "  🏁 Pool {}: 1000 transactions completed in {:?}",
                        i, duration
                    );
                }
                (i, duration)
            })
        })
        .collect();

    // Join all tasks and measure total time
    let mut task_results = Vec::new();
    for task in tasks {
        task_results.push(task.await?);
    }
    let total_execution_time = spawn_start.elapsed();

    println!("⚡ All tasks completed!");

    println!("\n📊 Performance Summary:");
    println!("{}", "-".repeat(60));

    if n <= 10 {
        for (pool_id, duration) in &task_results {
            println!(
                "  Pool {}: Average per transaction: {:?}",
                pool_id,
                *duration / 1000
            );
        }
        println!("{}", "-".repeat(60));
    }

    let total_task_duration: std::time::Duration = task_results.iter().map(|(_, d)| *d).sum();
    let avg_task_duration = total_task_duration / task_results.len() as u32;
    let fastest = task_results.iter().map(|(_, d)| *d).min().unwrap();
    let slowest = task_results.iter().map(|(_, d)| *d).max().unwrap();

    let total_transactions = n * 1000;
    let total_tx_per_sec = total_transactions as f64 / total_execution_time.as_secs_f64();

    println!("  📈 Pools: {}", n);
    println!("  ⏱️  Total wall-clock time: {:?}", total_execution_time);
    println!("  📊 Average pool duration: {:?}", avg_task_duration);
    println!("  🚀 Fastest pool: {:?}", fastest);
    println!("  🐌 Slowest pool: {:?}", slowest);
    println!("  💡 Avg per transaction: {:?}", avg_task_duration / 1000);
    println!(
        "  🔥 Total throughput: {:.2} tx/s ({} total transactions)",
        total_tx_per_sec, total_transactions
    );

    Ok(total_tx_per_sec)
}

async fn transactions_with_contention(
    cala: &CalaLedger,
    journal_id: JournalId,
    pool_size: usize,
    parallel_tasks: usize,
    n_account_sets: Option<usize>,
) -> anyhow::Result<f64> {
    // Setup phase: create one shared pool
    let setup_msg = match n_account_sets {
        Some(n) => format!(
            "🔧 Setting up shared pool with {} accounts and {} account sets...",
            pool_size, n
        ),
        None => format!("🔧 Setting up shared pool with {} accounts...", pool_size),
    };
    println!("{}", setup_msg);

    let mut shared_pool = Vec::new();

    for i in 0..pool_size {
        let (account1, _) = init_accounts(cala, false).await?;
        shared_pool.push(account1.id());
        println!("  ✓ Account {} created", i);
    }

    // Create account sets if requested and assign accounts round-robin
    if let Some(n_sets) = n_account_sets {
        let mut account_sets = Vec::new();

        // Create account sets
        for i in 0..n_sets {
            let account_set = NewAccountSet::builder()
                .id(AccountSetId::new())
                .name(format!("Contention Set {}", i))
                .journal_id(journal_id)
                .build()
                .unwrap();
            let created_set = cala.account_sets().create(account_set).await?;
            account_sets.push(created_set);
            println!("  ✓ Account set {} created", i);
        }

        // Assign accounts to sets round-robin
        for (account_idx, account_id) in shared_pool.iter().enumerate() {
            let set_idx = account_idx % n_sets;
            cala.account_sets()
                .add_member(account_sets[set_idx].id(), *account_id)
                .await?;
            println!("  ➤ Account {} assigned to set {}", account_idx, set_idx);
        }

        println!(
            "🎯 Setup complete. Starting {} concurrent tasks with shared pool and {} account sets...",
            parallel_tasks, n_sets
        );
    } else {
        println!(
            "🎯 Setup complete. Starting {} concurrent tasks with shared pool...",
            parallel_tasks
        );
    }

    // Spawn all tasks close together, each with a clone of the shared pool
    let spawn_start = std::time::Instant::now();

    let tasks: Vec<_> = (0..parallel_tasks)
        .map(|i| {
            let cala = cala.clone();
            let pool = shared_pool.clone(); // Clone the Vec directly
            tokio::spawn(async move {
                let start = std::time::Instant::now();
                for _ in 0..1000 {
                    execute_one_simple_transaction(&cala, journal_id, &pool)
                        .await
                        .unwrap();
                }
                let duration = start.elapsed();
                println!(
                    "  🏁 Task {}: 1000 transactions completed in {:?}",
                    i, duration
                );
                (i, duration)
            })
        })
        .collect();

    // Join all tasks and measure total time
    let mut task_results = Vec::new();
    for task in tasks {
        task_results.push(task.await?);
    }
    let total_execution_time = spawn_start.elapsed();

    println!("⚡ All tasks completed!");

    println!("\n📊 Contention Performance Summary:");
    println!("{}", "-".repeat(60));

    for (task_id, duration) in &task_results {
        println!(
            "  Task {}: Average per transaction: {:?}",
            task_id,
            *duration / 1000
        );
    }
    println!("{}", "-".repeat(60));

    let total_task_duration: std::time::Duration = task_results.iter().map(|(_, d)| *d).sum();
    let avg_task_duration = total_task_duration / task_results.len() as u32;
    let fastest = task_results.iter().map(|(_, d)| *d).min().unwrap();
    let slowest = task_results.iter().map(|(_, d)| *d).max().unwrap();

    let total_transactions = parallel_tasks * 1000;
    let total_tx_per_sec = total_transactions as f64 / total_execution_time.as_secs_f64();

    println!("  🏦 Shared pool size: {} accounts", pool_size);
    if let Some(n_sets) = n_account_sets {
        println!(
            "  🗂️  Account sets: {} (avg {:.1} accounts per set)",
            n_sets,
            pool_size as f64 / n_sets as f64
        );
    }
    println!("  🔀 Parallel tasks: {}", parallel_tasks);
    println!("  ⏱️  Total wall-clock time: {:?}", total_execution_time);
    println!("  📊 Average task duration: {:?}", avg_task_duration);
    println!("  🚀 Fastest task: {:?}", fastest);
    println!("  🐌 Slowest task: {:?}", slowest);
    println!("  💡 Avg per transaction: {:?}", avg_task_duration / 1000);
    println!(
        "  🔥 Total throughput: {:.2} tx/s ({} total transactions)",
        total_tx_per_sec, total_transactions
    );

    Ok(total_tx_per_sec)
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
