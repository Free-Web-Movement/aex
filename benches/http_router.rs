use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::sync::Arc;

use aex::http::router::{NodeType, Router as AexRouter};
use aex::http::params::SmallParams;
use aex::http::types::Executor;

fn bench_static_route(c: &mut Criterion) {
    let mut router = AexRouter::new(NodeType::Static("root".into()));
    
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
    router.get("/api/users", handler.clone());
    router.get("/api/posts", handler.clone());
    router.get("/api/comments", handler.clone());
    router.get("/static/app.js", handler.clone());
    
    let paths = vec![
        vec!["api", "users"],
        vec!["api", "posts"],
        vec!["api", "comments"],
        vec!["static", "app.js"],
    ];
    
    c.bench_function("static_route_4_paths", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &paths {
                black_box(router.match_route(path, &mut params));
            }
        });
    });
}

fn bench_param_route(c: &mut Criterion) {
    let mut router = AexRouter::new(NodeType::Static("root".into()));
    
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
    router.get("/api/users/:id", handler.clone());
    router.get("/api/posts/:post_id/comments/:comment_id", handler.clone());
    
    let paths = vec![
        vec!["api", "users", "123"],
        vec!["api", "posts", "456", "comments"],
    ];
    
    c.bench_function("param_route_2_paths", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &paths {
                black_box(router.match_route(path, &mut params));
            }
        });
    });
}

fn bench_wildcard_route(c: &mut Criterion) {
    let mut router = AexRouter::new(NodeType::Static("root".into()));
    
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
    router.get("/static/*", handler.clone());
    
    let paths = vec![
        vec!["static", "js", "app.js"],
        vec!["static", "css", "style.css"],
        vec!["static", "img", "logo.png"],
    ];
    
    c.bench_function("wildcard_route", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &paths {
                black_box(router.match_route(path, &mut params));
            }
        });
    });
}

fn bench_mixed_route(c: &mut Criterion) {
    let mut router = AexRouter::new(NodeType::Static("root".into()));
    
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
    
    router.get("/api/users", handler.clone());
    router.get("/api/users/:id", handler.clone());
    router.get("/api/posts/:post_id/comments/:comment_id", handler.clone());
    router.get("/static/*", handler.clone());
    router.get("/health", handler.clone());
    
    let static_paths = vec![vec!["api", "users"], vec!["health"]];
    let param_paths = vec![vec!["api", "users", "123"]];
    let multi_param_paths = vec![vec!["api", "posts", "456", "comments"]];
    let wildcard_paths = vec![vec!["static", "js", "app.js"]];
    
    c.bench_function("mixed_route_all_types", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            
            for path in &static_paths {
                black_box(router.match_route(path, &mut params));
            }
            params.clear();
            for path in &param_paths {
                black_box(router.match_route(path, &mut params));
            }
            params.clear();
            for path in &multi_param_paths {
                black_box(router.match_route(path, &mut params));
            }
            params.clear();
            for path in &wildcard_paths {
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

criterion_group!(benches, 
    bench_static_route, 
    bench_param_route, 
    bench_wildcard_route,
    bench_mixed_route,
    bench_hashmap_lookup
);
criterion_main!(benches);