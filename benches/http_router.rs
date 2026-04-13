use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::sync::Arc;

use aex::http::router::{NodeType, Router as AexRouter};
use aex::http::params::SmallParams;
use aex::http::types::Executor;

fn bench_aex_router_matching(c: &mut Criterion) {
    let mut router = AexRouter::new(NodeType::Static("root".into()));
    
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
    router.get("/api/users", handler.clone());
    router.get("/api/users/:id", handler.clone());
    router.get("/api/posts/:post_id/comments/:comment_id", handler.clone());
    router.get("/static/*", handler.clone());
    
    let paths = vec![
        vec!["api", "users"],
        vec!["api", "users", "123"],
        vec!["api", "posts", "456", "comments"],
        vec!["static", "js", "app.js"],
    ];
    
    c.bench_function("aex_router_match", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &paths {
                black_box(router.match_route(path, &mut params));
            }
        });
    });
}

fn bench_hashmap_lookup(c: &mut Criterion) {
    let iterations = 100_000;
    
    let keys: Vec<String> = vec!["key1", "key2", "key3", "key4", "key5"].into_iter().map(|s| s.to_string()).collect();
    
    let std_map: HashMap<String, i32> = keys.iter().enumerate().map(|(i, k)| (k.clone(), i as i32)).collect();
    let ahash_map: ahash::AHashMap<String, i32> = keys.iter().enumerate().map(|(i, k)| (k.clone(), i as i32)).collect();
    
    c.bench_function("std_hashmap_lookup", |b| {
        let keys = &keys;
        b.iter(|| {
            for _ in 0..iterations {
                for key in keys {
                    black_box(std_map.get(key));
                }
            }
        });
    });
    
    c.bench_function("ahash_lookup", |b| {
        let keys = &keys;
        b.iter(|| {
            for _ in 0..iterations {
                for key in keys {
                    black_box(ahash_map.get(key));
                }
            }
        });
    });
}

fn bench_string_allocation(c: &mut Criterion) {
    let path = "/api/users/123/profile";
    
    c.bench_function("string_to_string", |b| {
        b.iter(|| black_box(path.to_string()));
    });
    
    c.bench_function("string_from", |b| {
        b.iter(|| black_box(String::from(path)));
    });
}

criterion_group!(benches, bench_aex_router_matching, bench_hashmap_lookup, bench_string_allocation);
criterion_main!(benches);