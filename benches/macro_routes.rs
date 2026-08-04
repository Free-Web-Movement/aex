//! 宏注册 vs 手动注册：路由匹配性能对比。
//!
//! 验证 `#[aex::routes]` + `router.push(实例)` 注册的路由，在请求期
//! 与手动 `router.insert(...)` 注册的路由性能一致。

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;

use aex::connection::context::Context;
use aex::http::params::SmallParams;
use aex::http::router::{AexRoutes, Router};
use aex::http::types::Executor;

// 宏注册的模块
struct User {
    name: String,
}

#[aex::routes]
impl User {
    // 多路径
    #[get(["/", "/profile"])]
    fn profile(&self, ctx: &mut Context) {
        ctx.text(&self.name);
    }

    // async + 参数路径
    #[get("/users/:id")]
    async fn user(&self, ctx: &mut Context) {
        ctx.text("user");
    }

    // POST
    #[post("/resources")]
    fn create(&self, ctx: &mut Context) {
        ctx.text("created");
    }
}

fn bench_macro_vs_manual(c: &mut Criterion) {
    let handler: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));

    // 宏注册：一次 push 挂载全部
    let mut macro_router = Router::default();
    macro_router.push(User { name: "aex".into() });

    // 手动注册等价路径
    let mut manual_router = Router::default();
    manual_router.insert("/", Some("GET"), handler.clone(), None);
    manual_router.insert("/profile", Some("GET"), handler.clone(), None);
    manual_router.insert("/users/:id", Some("GET"), handler.clone(), None);
    manual_router.insert("/resources", Some("POST"), handler.clone(), None);

    let static_paths: Vec<Vec<&str>> = vec![vec![], vec!["profile"]];
    let param_paths: Vec<Vec<&str>> = vec![vec!["users", "42"]];

    c.bench_function("macro_routes_static_match", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &static_paths {
                black_box(macro_router.match_route(path, &mut params));
            }
        });
    });

    c.bench_function("manual_routes_static_match", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &static_paths {
                black_box(manual_router.match_route(path, &mut params));
            }
        });
    });

    c.bench_function("macro_routes_param_match", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &param_paths {
                black_box(macro_router.match_route(path, &mut params));
            }
        });
    });

    c.bench_function("manual_routes_param_match", |b| {
        b.iter(|| {
            let mut params = SmallParams::default();
            for path in &param_paths {
                black_box(manual_router.match_route(path, &mut params));
            }
        });
    });
}

criterion_group!(benches, bench_macro_vs_manual);
criterion_main!(benches);
