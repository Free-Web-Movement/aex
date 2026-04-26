use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;

fn bench_middleware_overhead(c: &mut Criterion) {
    use aex::http::middlewares::rate_limit::RateLimitConfig;
    use aex::http::types::Executor;

    c.bench_function("rate_limit_middleware_build", |b| {
        b.iter(|| {
            let rate_limit = RateLimitConfig::new(1000, 60).build();
            black_box(rate_limit);
        });
    });
}

fn bench_logger_middleware(c: &mut Criterion) {
    use aex::http::middlewares::logger::LogConfig;

    c.bench_function("logger_middleware_build", |b| {
        b.iter(|| {
            let l = LogConfig::new().all().build();
            black_box(l);
        });
    });
}

fn bench_cors_middleware(c: &mut Criterion) {
    use aex::http::middlewares::cors::CorsConfig;

    c.bench_function("cors_config_build", |b| {
        b.iter(|| {
            let config = CorsConfig::new().allow_origin_all(true).max_age(3600);
            black_box(config);
        });
    });
}

fn bench_middleware_chaining(c: &mut Criterion) {
    use aex::http::types::Executor;

    c.bench_function("middleware_chain_3", |b| {
        b.iter(|| {
            let mut chain = Vec::new();
            for _ in 0..3 {
                let middleware: Arc<Executor> = Arc::new(|_ctx| Box::pin(async { true }));
                chain.push(middleware);
            }
            black_box(chain);
        });
    });
}

criterion_group!(
    benches,
    bench_middleware_overhead,
    bench_logger_middleware,
    bench_cors_middleware,
    bench_middleware_chaining
);
criterion_main!(benches);
