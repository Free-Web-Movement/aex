use std::sync::Arc;

use aex::connection::context::Context;
use aex::http::params::SmallParams;
use aex::http::router::{AexRoutes, NodeType, Router};
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

#[test]
fn test_node_type_variant_checks() {
    let s = NodeType::Static("users".to_string());
    assert!(s.is_static());
    assert!(!s.is_param());
    assert!(!s.is_wildcard());

    let p = NodeType::Param("id".to_string());
    assert!(p.is_param());
    assert!(!p.is_static());
    assert!(!p.is_wildcard());

    let w = NodeType::Wildcard;
    assert!(w.is_wildcard());
    assert!(!w.is_static());
    assert!(!w.is_param());

    match NodeType::default() {
        NodeType::Static(name) => assert!(name.is_empty()),
        other => panic!("default should be empty Static, got {other:?}"),
    }
}

#[test]
fn test_match_route_static() {
    let mut router = Router::default();
    router.insert("/api/users", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/api/users"), &mut params)
        .expect("static route should match");
    assert!(node.handlers.is_some());
    assert!(params.is_empty());
}

#[test]
fn test_match_route_param_capture() {
    let mut router = Router::default();
    router.insert("/user/:id/posts/:pid", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/user/42/posts/7"), &mut params)
        .expect("param route should match");
    assert!(node.handlers.is_some());
    assert_eq!(params.get("id"), Some("42"));
    assert_eq!(params.get("pid"), Some("7"));
}

#[test]
fn test_match_route_static_beats_param() {
    let mut router = Router::default();
    router.insert("/data/:x", Some("GET"), ok_handler(), None);
    router.insert("/data/latest", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/data/latest"), &mut params)
        .expect("static should win over param");
    assert!(node.handlers.is_some());
    assert!(params.is_empty());
}

#[test]
fn test_match_route_wildcard_matches_rest() {
    let mut router = Router::default();
    router.insert("/static/*", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/static/css/main.css"), &mut params)
        .expect("wildcard should match");
    assert!(node.handlers.is_some());
    assert_eq!(params.get("*"), Some("css/main.css"));
}

#[test]
fn test_match_route_wildcard_backtrack_to_ancestor() {
    let mut router = Router::default();
    router.insert("/files/*", Some("GET"), ok_handler(), None);
    router.insert("/files/a/b", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/files/a/x"), &mut params)
        .expect("backtrack to ancestor wildcard");
    assert!(node.handlers.is_some());
    assert_eq!(params.get("*"), Some("a/x"));
}

#[test]
fn test_match_route_wildcard_at_root() {
    let mut router = Router::default();
    router.insert("/*", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    let node = router
        .match_route(&segs("/foo/bar"), &mut params)
        .expect("root wildcard should match");
    assert!(node.handlers.is_some());
    assert_eq!(params.get("*"), Some("foo/bar"));
}

#[test]
fn test_match_route_no_match_returns_none() {
    let mut router = Router::default();
    router.insert("/a/b", Some("GET"), ok_handler(), None);

    let mut params = SmallParams::new();
    assert!(router.match_route(&segs("/a/c"), &mut params).is_none());
    assert!(router.match_route(&segs("/b"), &mut params).is_none());
}

#[test]
fn test_match_route_empty_path_returns_root() {
    let router = Router::default();
    let mut params = SmallParams::new();
    let node = router
        .match_route(&[], &mut params)
        .expect("empty path matches root");
    assert!(node.handlers.is_none());
}

#[test]
fn test_has_route_strips_query_and_normalizes() {
    let mut router = Router::default();
    router.get("/api/user/:id", ok_handler());

    assert!(router.has_route("GET", "/api/user/1"));
    assert!(router.has_route("get", "/api/user/1?tab=posts&page=2"));
    assert!(router.has_route("GET", "//api//user//1"));
}

#[test]
fn test_has_route_wildcard_method_fallback() {
    let mut router = Router::default();
    router.insert("/universal", None, ok_handler(), None);

    assert!(router.has_route("GET", "/universal"));
    assert!(router.has_route("POST", "/universal"));
    assert!(router.has_route("DELETE", "/universal"));
    assert!(router.has_route("TRACE", "/universal"));
}

#[test]
fn test_has_route_false_cases() {
    let mut router = Router::default();
    router.get("/only-get", ok_handler());

    assert!(!router.has_route("GET", "/missing"));
    assert!(!router.has_route("POST", "/only-get"));
    assert!(!router.has_route("GET", "/only-get/extra"));
}

#[test]
fn test_fluent_methods_register() {
    let mut router = Router::default();
    router.post("/p", ok_handler());
    router.put("/put", ok_handler());
    router.delete("/del", ok_handler());
    router.patch("/patch", ok_handler());
    router.options("/opt", ok_handler());
    router.head("/head", ok_handler());
    router.all("/any", ok_handler());

    assert!(router.has_route("POST", "/p"));
    assert!(router.has_route("PUT", "/put"));
    assert!(router.has_route("DELETE", "/del"));
    assert!(router.has_route("PATCH", "/patch"));
    assert!(router.has_route("OPTIONS", "/opt"));
    assert!(router.has_route("HEAD", "/head"));
    assert!(router.has_route("TRACE", "/any"));
}

#[test]
fn test_fluent_with_middlewares_registered() {
    let mut router = Router::default();
    let mw = aex::_sync!(|_| true);

    router.post_with("/p", vec![mw.clone()], ok_handler());
    router.put_with("/put", [mw.clone()], ok_handler());
    router.delete_with("/del", vec![mw.clone()], ok_handler());
    router.patch_with("/patch", vec![mw.clone()], ok_handler());
    router.options_with("/opt", vec![mw.clone()], ok_handler());
    router.head_with("/head", vec![mw.clone()], ok_handler());
    router.all_with("/any", vec![mw.clone()], ok_handler());

    let mut params = SmallParams::new();
    let node = router.match_route(&segs("/p"), &mut params).unwrap();
    let mws = node.middlewares.as_ref().expect("post_with middleware missing");
    assert_eq!(mws.get("POST").map(|v| v.len()), Some(1));

    let node = router.match_route(&segs("/any"), &mut params).unwrap();
    let mws = node.middlewares.as_ref().expect("all_with middleware missing");
    assert_eq!(mws.get("*").map(|v| v.len()), Some(1));
}

struct DummyApi {
    name: String,
}

impl AexRoutes for DummyApi {
    fn __aex_register(router: &mut Router, this: Arc<Self>) {
        let name = this.name.clone();
        router.get("/greet", move |ctx: &mut Context| {
            ctx.text(format!("hello {name}"));
        });
        router.post("/greet", |_ctx: &mut Context| true);
    }
}

#[test]
fn test_push_registers_instance_routes() {
    let mut router = Router::default();
    router.push(DummyApi {
        name: "aex".to_string(),
    });
    assert!(router.has_route("GET", "/greet"));
    assert!(router.has_route("POST", "/greet"));
}

#[cfg(feature = "router-cache")]
#[test]
fn test_finalize_recurses_whole_tree() {
    let mut router = Router::default();
    router.insert("/api/:version/users/*", Some("GET"), ok_handler(), None);
    router.finalize();
    assert!(router.has_route("GET", "/api/v1/users/1/2"));
}

#[test]
fn test_static_files_normalizes_prefix_without_slash() {
    let dir = std::env::temp_dir();
    let mut router = Router::default();
    router.static_files_with("static", StaticFiles::new(dir));

    assert!(router.has_route("GET", "/static/anything.txt"));
    assert!(router.has_route("GET", "/static"));
}
