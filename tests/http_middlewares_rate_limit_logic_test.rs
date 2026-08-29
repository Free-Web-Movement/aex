use aex::{
    connection::{context::Context, global::GlobalContext},
    http::{
        meta::HttpMetadata,
        middlewares::rate_limit::RateLimitConfig,
        protocol::{header::HeaderKey, status::StatusCode},
    },
};
use std::{net::SocketAddr, sync::Arc, time::Duration};

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port).parse().unwrap()
}

fn make_ctx(port: u16) -> Context {
    let a = addr(port);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    ctx.local.set_value(HttpMetadata::new());
    ctx
}

fn limit(meta: &HttpMetadata) -> &str {
    meta.headers
        .get(&HeaderKey::from("X-RateLimit-Limit"))
        .unwrap()
}
fn remaining(meta: &HttpMetadata) -> &str {
    meta.headers
        .get(&HeaderKey::from("X-RateLimit-Remaining"))
        .unwrap()
}

#[tokio::test]
async fn rate_limit_consumes_tokens_then_rejects_429() {
    let mut ctx = make_ctx(9100);
    let executor = RateLimitConfig::new(2, 60).build();

    assert!(executor(&mut ctx).await);
    assert_eq!(
        remaining(ctx.local.get_ref::<HttpMetadata>().unwrap()),
        "1"
    );
    assert!(executor(&mut ctx).await);
    assert_eq!(
        remaining(ctx.local.get_ref::<HttpMetadata>().unwrap()),
        "0"
    );

    assert!(!executor(&mut ctx).await);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(limit(meta), "2");
    assert_eq!(meta.status, StatusCode::TooManyRequests);
    assert!(String::from_utf8_lossy(&meta.body).contains("Rate limit exceeded"));
    let retry: u64 = meta
        .headers
        .get(&HeaderKey::RetryAfter)
        .unwrap()
        .parse()
        .unwrap();
    assert!(retry > 0);
}

#[tokio::test]
async fn rate_limit_window_zero_always_refills() {
    let mut ctx = make_ctx(9101);
    let executor = RateLimitConfig::new(1, 0).build();
    for _ in 0..5 {
        assert!(executor(&mut ctx).await);
    }
    assert_eq!(
        remaining(ctx.local.get_ref::<HttpMetadata>().unwrap()),
        "0"
    );
}

#[tokio::test]
async fn rate_limit_refills_after_window_elapses() {
    let mut ctx = make_ctx(9102);
    let executor = RateLimitConfig::new(1, 1).build();
    assert!(executor(&mut ctx).await);
    assert!(!executor(&mut ctx).await);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(executor(&mut ctx).await);
}

#[tokio::test]
async fn rate_limit_by_header_separates_buckets() {
    let mut ctx = make_ctx(9103);
    let executor = RateLimitConfig::new(1, 60).by_header("X-API-Key").build();

    ctx.local
        .get_mut::<HttpMetadata>()
        .unwrap()
        .headers
        .insert(HeaderKey::from("X-API-Key"), "key-a");
    assert!(executor(&mut ctx).await);
    assert!(!executor(&mut ctx).await);

    ctx.local
        .get_mut::<HttpMetadata>()
        .unwrap()
        .headers
        .insert(HeaderKey::from("X-API-Key"), "key-b");
    assert!(executor(&mut ctx).await);
}

#[tokio::test]
async fn rate_limit_by_header_missing_value_uses_unknown_key() {
    let mut ctx = make_ctx(9104);
    let executor = RateLimitConfig::new(1, 60).by_header("X-API-Key").build();
    assert!(executor(&mut ctx).await);
    assert!(!executor(&mut ctx).await);
}

#[tokio::test]
async fn rate_limit_by_ip_uses_addr_as_key() {
    let mut ctx_a = make_ctx(9105);
    let mut ctx_b = make_ctx(9106);
    let executor = RateLimitConfig::new(1, 60).by_ip().build();
    assert!(executor(&mut ctx_a).await);
    assert!(!executor(&mut ctx_a).await);
    assert!(executor(&mut ctx_b).await);
}

#[tokio::test]
async fn rate_limit_by_path_uses_path_plus_addr_as_key() {
    let mut ctx_a = make_ctx(9107);
    let mut ctx_b = make_ctx(9108);
    let executor = RateLimitConfig::new(1, 60).by_path().build();
    ctx_a.local.get_mut::<HttpMetadata>().unwrap().path = "/api/a".into();
    ctx_b.local.get_mut::<HttpMetadata>().unwrap().path = "/api/a".into();
    assert!(executor(&mut ctx_a).await);
    assert!(executor(&mut ctx_b).await);

    let mut ctx_a2 = make_ctx(9107);
    ctx_a2.local.get_mut::<HttpMetadata>().unwrap().path = "/api/b".into();
    assert!(executor(&mut ctx_a2).await);
}

#[tokio::test]
async fn rate_limit_macro_executes_end_to_end() {
    let mut ctx = make_ctx(9109);
    let executor = aex::rate_limit!(1, 60);
    assert!(executor(&mut ctx).await);
    assert!(!executor(&mut ctx).await);
}
