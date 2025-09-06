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
            init_accounts_with_account_sets_depth(&cala, &journal, false, 1)
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
            init_accounts_with_account_sets_depth(&cala, &journal, false, 5)
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

criterion_group!(
    benches,
    post_simple_transaction,
    post_multi_layer_transaction,
    post_simple_transaction_with_effective_balances,
    post_simple_transaction_with_one_account_set,
    post_simple_transaction_with_five_account_set,
    post_simple_transaction_with_velocity,
    post_simple_transaction_with_skipped_velocity,
    post_simple_transaction_with_hit_velocity,
);
criterion_main!(benches);
