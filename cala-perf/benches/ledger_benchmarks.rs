use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use tokio::runtime::Runtime;

use cala_perf::{init_accounts, init_cala, init_journal, templates::simple_transfer};

fn post_simple_transaction(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let (cala, journal, sender, recipient) = rt.block_on(async {
        let cala = init_cala().await.unwrap();
        simple_transfer::init(&cala).await.unwrap();
        let journal = init_journal(&cala).await.unwrap();
        let (sender, recipient) = init_accounts(&cala).await.unwrap();
        (cala, journal, sender, recipient)
    });

    c.bench_function("post_simple_transaction", |b| {
        b.to_async(&rt).iter(|| async {
            simple_transfer::execute(
                black_box(&cala),
                black_box(journal.id()),
                black_box(sender.id()),
                black_box(recipient.id()),
            ).await.unwrap()
        })
    });
}

criterion_group!(benches, post_simple_transaction);
criterion_main!(benches);