use criterion::{criterion_group, criterion_main, Criterion};
use tokio::runtime::{Builder, Runtime};

use std::hint::black_box;

use cala_perf::{
    attach_velocity_to_account, init_accounts, init_accounts_with_account_sets_depth, init_cala,
    init_journal,
    templates::{multi_layer_template, simple_transfer},
};

fn create_single_worker_runtime() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

fn post_simple_transaction(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, false).await.unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("1. post_simple_transaction", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_multi_layer_transaction(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        multi_layer_template::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, false).await.unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("2. post_multi_layer_transaction", |b| {
        b.to_async(&rt).iter(|| async {
            multi_layer_template::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_simple_transaction_with_effective_balances(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, true).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, false).await.unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("3. post_simple_transaction_with_effective_balances", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

/// Days of existing daily history under the backdated posting.
const BACKDATED_HISTORY_DAYS: i64 = 365;

/// Posts at the oldest date of a pair that has one effective-balance row per
/// day for `BACKDATED_HISTORY_DAYS`, so each iteration adjusts every later
/// row of both accounts (a set-based UPDATE, no balance rows loaded).
fn post_backdated_transaction_with_effective_balances(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient, oldest) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, true).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, false).await.unwrap();
        let today = chrono::Utc::now().date_naive();
        let oldest = today - chrono::Duration::days(BACKDATED_HISTORY_DAYS - 1);
        for day in 0..BACKDATED_HISTORY_DAYS {
            simple_transfer::execute_effective(
                &cala,
                journal.id(),
                sender.id(),
                recipient.id(),
                oldest + chrono::Duration::days(day),
            )
            .await
            .unwrap();
        }
        (cala, journal, sender, recipient, oldest)
    });

    c.bench_function(
        "10. post_backdated_transaction_with_effective_balances_365_days",
        |b| {
            b.to_async(&rt).iter(|| async {
                simple_transfer::execute_effective(
                    black_box(&cala),
                    black_box(journal.id()),
                    black_box(sender.id()),
                    black_box(recipient.id()),
                    black_box(oldest),
                )
                .await
                .unwrap()
            })
        },
    );
}

fn post_simple_transaction_with_velocity(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, true).await.unwrap();
        attach_velocity_to_account(&cala, sender.id(), 100_000_000)
            .await
            .unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("4. post_simple_transaction_with_velocity", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_simple_transaction_with_skipped_velocity(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, false).await.unwrap();
        attach_velocity_to_account(&cala, sender.id(), 0)
            .await
            .unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("4. post_simple_transaction_with_skipped_velocity", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_simple_transaction_with_hit_velocity(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient) = init_accounts(&cala, true).await.unwrap();
        attach_velocity_to_account(&cala, sender.id(), 0)
            .await
            .unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("6. post_simple_transaction_with_hit_velocity", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap_err();
        })
    });
}

fn post_simple_transaction_with_one_account_set(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient, _sender_set, _recipient_set) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient, sender_set, recipient_set) =
            init_accounts_with_account_sets_depth(&cala, &journal, false, 1, false)
                .await
                .unwrap();
        (cala, journal, sender, recipient, sender_set, recipient_set)
    });

    c.bench_function("7. post_simple_transaction_with_one_account_set", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_simple_transaction_with_five_account_set(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient, _sender_set, _recipient_set) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient, sender_set, recipient_set) =
            init_accounts_with_account_sets_depth(&cala, &journal, false, 5, false)
                .await
                .unwrap();
        (cala, journal, sender, recipient, sender_set, recipient_set)
    });

    c.bench_function("8. post_simple_transaction_with_five_account_sets", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

fn post_simple_transaction_with_ec_account_set(c: &mut Criterion) {
    let rt = create_single_worker_runtime();

    let (cala, journal, sender, recipient, _sender_set, _recipient_set) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala, false).await.unwrap();
        let (sender, recipient, sender_set, recipient_set) =
            init_accounts_with_account_sets_depth(&cala, &journal, false, 1, true)
                .await
                .unwrap();
        (cala, journal, sender, recipient, sender_set, recipient_set)
    });

    c.bench_function("9. post_simple_transaction_with_ec_account_set", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            )
            .await
            .unwrap()
        })
    });
}

criterion_group!(
    benches,
    post_simple_transaction,
    post_multi_layer_transaction,
    post_simple_transaction_with_effective_balances,
    post_backdated_transaction_with_effective_balances,
    post_simple_transaction_with_one_account_set,
    post_simple_transaction_with_five_account_set,
    post_simple_transaction_with_velocity,
    post_simple_transaction_with_skipped_velocity,
    post_simple_transaction_with_hit_velocity,
    post_simple_transaction_with_ec_account_set,
);
criterion_main!(benches);
