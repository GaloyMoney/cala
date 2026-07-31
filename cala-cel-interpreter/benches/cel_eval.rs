use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

use cala_cel_interpreter::{CelContext, CelExpression, CelMap};

fn velocity_like_context() -> CelContext {
    let mut entry = CelMap::new();
    entry.insert("units", rust_decimal::Decimal::new(100, 0));
    let mut vars = CelMap::new();
    vars.insert("entry", entry);
    let mut context = CelMap::new();
    context.insert("vars", vars);
    let mut ctx = CelContext::new();
    ctx.add_variable("context", context);
    ctx
}

fn params_context() -> CelContext {
    let mut hello = CelMap::new();
    hello.insert("world", 42);
    let mut params = CelMap::new();
    params.insert("hello", hello);
    let mut ctx = CelContext::new();
    ctx.add_variable("params", params);
    ctx
}

fn bench_parse_cached(c: &mut Criterion) {
    let exprs = [
        "context.vars.entry.units == decimal('100')",
        "params.hello.world",
        "date('2022-10-10')",
    ];
    // warm the caches
    for e in exprs {
        let _ = e.parse::<CelExpression>().unwrap();
    }
    c.bench_function("parse_cached (cache hit)", |b| {
        let mut i = 0;
        b.iter(|| {
            let e = exprs[i % exprs.len()];
            i += 1;
            black_box(e).parse::<CelExpression>().unwrap()
        })
    });
}

fn bench_evaluate(c: &mut Criterion) {
    let cases: &[(&str, &str, CelContext)] = &[
        ("eval_lookup", "params.hello.world", params_context()),
        (
            "eval_decimal_eq (velocity-style)",
            "context.vars.entry.units == decimal('100')",
            velocity_like_context(),
        ),
        (
            "eval_logic",
            "true || false ? false && true : true",
            CelContext::new(),
        ),
        ("eval_date_fn", "date('2022-10-10')", CelContext::new()),
    ];

    for (name, src, ctx) in cases {
        let expr = src.parse::<CelExpression>().unwrap();
        c.bench_function(name, |b| b.iter(|| expr.evaluate(black_box(ctx)).unwrap()));
    }
}

criterion_group!(benches, bench_parse_cached, bench_evaluate);
criterion_main!(benches);
