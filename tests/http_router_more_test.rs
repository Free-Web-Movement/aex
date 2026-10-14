//! Extra coverage for `protocols::http::router` execution paths: `on_request`
//! middleware/handler dispatch, form-body reading, 404 handling, `handle`
//! keep-alive loops, `is_http`, plus a few registration/feature branches the
//! existing suites leave open. Complements `http_router_test.rs` /
//! `http_router_gap_test.rs`.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Mutex;

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::connection::global::GlobalContext;
use aex::http::meta::HttpMetadata;
use aex::http::params::SmallParams;
use aex::http::protocol::content_type::ContentType;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::method::HttpMethod;
use aex::http::protocol::status::StatusCode;
use aex::http::router::Router;
use aex::http::static_files::StaticFiles;
use aex::http::types::Executor;

fn ok_handler() -> Arc<Executor> {
    aex::_sync!(|_| true)
}

fn segs(path: &str) -> Vec<&str> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

fn global() -> Arc<GlobalContext> {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    Arc::new(GlobalContext::new(addr, None))
}

fn ctx_with(meta: HttpMetadata, reader: Option<BoxReader>, writer: Option<BoxWriter>) -> Context {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let mut ctx = Context::new(reader, writer, global(), addr);
    ctx.set(meta);
    ctx
}

fn cursor_reader(bytes: &'static [u8]) -> BoxReader {
    Box::new(tokio::io::BufReader::new(std::io::Cursor::new(bytes)))
}

#[tokio::test]
async fn on_request_without_metadata_returns_false() {
    let router = Router::default();
    let mut ctx = Context::new(None, None, global(), "127.0.0.1:1".parse().unwrap());
    assert!(!router.on_request(&mut ctx).await);
}

#[tokio::test]
async fn on_request_missing_route_sets_404() {
    let mut router = Router::default();
    router.get("/present", ok_handler());

    let mut meta = HttpMetadata::default();
    meta.path = "/absent".to_string();
    let mut ctx = ctx_with(meta, None, None);

    assert!(router.on_request(&mut ctx).await);
    assert_eq!(
        ctx.local.get_ref::<HttpMetadata>().unwrap().status,
        StatusCode::NotFound
    );
}

#[tokio::test]
async fn on_request_runs_handler_and_strips_query() {
    let mut router = Router::default();
    router.get("/hello", |ctx: &mut Context| {
        ctx.text("hi");
    });

    let mut meta = HttpMetadata::default();
    meta.path = "/hello?x=1&y=2".to_string();
    let mut ctx = ctx_with(meta, None, None);

    assert!(router.on_request(&mut ctx).await);
    // Handler wrote a body into the metadata.
    assert_eq!(ctx.local.get_ref::<HttpMetadata>().unwrap().body, b"hi");
}

#[tokio::test]
async fn on_request_captures_params_and_wildcard() {
    let mut router = Router::default();
    router.get("/user/:id/*", ok_handler());

    let mut meta = HttpMetadata::default();
    meta.path = "/user/42/a/b".to_string();
    let mut ctx = ctx_with(meta, None, None);

    assert!(router.on_request(&mut ctx).await);
    let params = ctx.local.get_ref::<HttpMetadata>().unwrap().params.clone();
    let params = params.expect("params set");
    let id = params.data.as_ref().and_then(|d| d.get("id"));
    assert_eq!(id.map(|s| s.as_str()), Some("42"));
}

#[tokio::test]
async fn on_request_method_wildcard_fallback_runs_handler() {
    let mut router = Router::default();
    router.insert("/any", None, ok_handler(), None);

    let mut meta = HttpMetadata::default();
    meta.path = "/any".to_string();
    meta.method = HttpMethod::POST;
    let mut ctx = ctx_with(meta, None, None);

    assert!(router.on_request(&mut ctx).await);
}

#[tokio::test]
async fn on_request_middleware_failure_sets_400() {
    let mut router = Router::default();
    router.get_with("/guarded", [aex::_sync!(|_| false)], ok_handler());

    let mut meta = HttpMetadata::default();
    meta.path = "/guarded".to_string();
    let mut ctx = ctx_with(meta, None, None);

    assert!(!router.on_request(&mut ctx).await);
    assert_eq!(
        ctx.local.get_ref::<HttpMetadata>().unwrap().status,
        StatusCode::BadRequest
    );
}

#[tokio::test]
async fn on_request_reads_form_body_from_reader() {
    let mut router = Router::default();
    router.post("/form", |ctx: &mut Context| {
        ctx.text("ok");
    });

    let body = b"a=1&b=2";
    let mut meta = HttpMetadata::default();
    meta.path = "/form".to_string();
    meta.method = HttpMethod::POST;
    meta.content_type = ContentType::parse("application/x-www-form-urlencoded");
    meta.headers
        .insert(HeaderKey::ContentLength, body.len().to_string());

    let mut ctx = ctx_with(meta, Some(cursor_reader(body)), None);
    assert!(router.on_request(&mut ctx).await);
}

#[tokio::test]
async fn on_request_form_without_reader_returns_false() {
    let mut router = Router::default();
    router.post("/form", ok_handler());

    let mut meta = HttpMetadata::default();
    meta.path = "/form".to_string();
    meta.method = HttpMethod::POST;
    meta.content_type = ContentType::parse("application/x-www-form-urlencoded");
    meta.headers
        .insert(HeaderKey::ContentLength, "3".to_string());

    let mut ctx = ctx_with(meta, None, None);
    assert!(!router.on_request(&mut ctx).await);
}

#[tokio::test]
async fn handle_serves_request_then_breaks_on_eof() {
    let mut router = Router::default();
    router.get("/x", |ctx: &mut Context| {
        ctx.text("ok");
    });
    let router = Arc::new(router);

    let req = b"GET /x HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        Some(cursor_reader(req)),
        Some(Box::new(tokio::io::sink())),
    )));

    router.handle(ctx).await.unwrap();
}

#[tokio::test]
async fn handle_honours_http11_connection_close() {
    let mut router = Router::default();
    router.get("/x", ok_handler());
    let router = Arc::new(router);

    let req = b"GET /x HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        Some(cursor_reader(req)),
        Some(Box::new(tokio::io::sink())),
    )));

    router.handle(ctx).await.unwrap();
}

#[tokio::test]
async fn handle_honours_http10_keep_alive_header() {
    let mut router = Router::default();
    router.get("/x", ok_handler());
    let router = Arc::new(router);

    let req = b"GET /x HTTP/1.0\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        Some(cursor_reader(req)),
        Some(Box::new(tokio::io::sink())),
    )));

    router.handle(ctx).await.unwrap();
}

#[tokio::test]
async fn is_http_true_for_http_request_and_serves_it() {
    let mut router = Router::default();
    router.get("/x", |ctx: &mut Context| {
        ctx.text("ok");
    });
    let router = Arc::new(router);

    let req = b"GET /x HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        Some(cursor_reader(req)),
        Some(Box::new(tokio::io::sink())),
    )));

    assert!(Router::is_http(router, ctx).await.unwrap());
}

#[tokio::test]
async fn is_http_false_for_plain_tcp_payload() {
    let router = Arc::new(Router::default());
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        Some(cursor_reader(b"\x01\x02\x03not-http")),
        Some(Box::new(tokio::io::sink())),
    )));

    assert!(!Router::is_http(router, ctx).await.unwrap());
}

#[tokio::test]
async fn is_http_false_without_reader() {
    let router = Arc::new(Router::default());
    let ctx = Arc::new(Mutex::new(ctx_with(
        HttpMetadata::default(),
        None,
        Some(Box::new(tokio::io::sink())),
    )));

    assert!(!Router::is_http(router, ctx).await.unwrap());
}

#[test]
fn get_with_registers_middleware() {
    let mut router = Router::default();
    router.get_with("/g", [aex::_sync!(|_| true)], ok_handler());

    let mut params = SmallParams::new();
    let node = router.match_route(&segs("/g"), &mut params).unwrap();
    assert!(node.middlewares.as_ref().unwrap().contains_key("GET"));
}

#[test]
fn static_files_registers_prefix_routes() {
    let mut router = Router::default();
    router.static_files("/assets", std::env::temp_dir());
    assert!(router.has_route("GET", "/assets"));
    assert!(router.has_route("GET", "/assets/app.js"));
}

#[cfg(feature = "router-cache")]
#[test]
fn finalize_recurses_param_wildcard_and_statics() {
    let mut router = Router::default();
    router.insert("/a/:id", Some("GET"), ok_handler(), None);
    router.insert("/b/*", Some("GET"), ok_handler(), None);
    router.get("/c/d", ok_handler());
    router.finalize();
    assert!(router.has_route("GET", "/a/1"));
    assert!(router.has_route("GET", "/b/x/y"));
    assert!(router.has_route("GET", "/c/d"));
}

#[test]
fn static_files_with_default_config_builds_handler() {
    let dir = std::env::temp_dir();
    let mut router = Router::default();
    router.static_files_with("/pub", StaticFiles::new(dir));
    assert!(router.has_route("GET", "/pub"));
}
