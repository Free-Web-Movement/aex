//! Router Performance Comparison: AEx vs Axum vs Actix-web

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::collections::HashMap;
use std::sync::Arc;

use aex::http::params::SmallParams;
use aex::http::router::{NodeType, Router as AexRouter};
use aex::http::types::Executor;

fn bench_aex_router(c: &mut Criterion) {
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));

    // Static route
    c.bench_function("aex_static", |b| {
        let mut router = AexRouter::new(NodeType::Static("root".into()));
        router.get("/api/users", handler.clone());

        let path = vec!["api", "users"];
        b.iter(|| {
            let mut params = SmallParams::default();
            black_box(router.match_route(&path, &mut params));
        });
    });

    // Param route
    c.bench_function("aex_param", |b| {
        let mut router = AexRouter::new(NodeType::Static("root".into()));
        router.get("/api/users/:id", handler.clone());

        let path = vec!["api", "users", "123"];
        b.iter(|| {
            let mut params = SmallParams::default();
            black_box(router.match_route(&path, &mut params));
        });
    });

    // Wildcard
    c.bench_function("aex_wildcard", |b| {
        let mut router = AexRouter::new(NodeType::Static("root".into()));
        router.get("/static/*", handler.clone());

        let path = vec!["static", "js", "app.js"];
        b.iter(|| {
            let mut params = SmallParams::default();
            black_box(router.match_route(&path, &mut params));
        });
    });
}

fn bench_hashmap(c: &mut Criterion) {
    let keys: Vec<String> = (0..10).map(|i| format!("key{}", i)).collect();

    c.bench_function("ahash_10", |b| {
        let mut map = ahash::AHashMap::new();
        for (i, k) in keys.iter().enumerate() {
            map.insert(k.clone(), i);
        }
        b.iter(|| {
            for key in &keys {
                black_box(map.get(key));
            }
        });
    });

    c.bench_function("std_hashmap_10", |b| {
        let mut map = HashMap::new();
        for (i, k) in keys.iter().enumerate() {
            map.insert(k.clone(), i);
        }
        b.iter(|| {
            for key in &keys {
                black_box(map.get(key));
            }
        });
    });
}

criterion_group!(benches, bench_aex_router, bench_hashmap);
criterion_main!(benches);
