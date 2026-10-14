//! Additional coverage for `aex::connection::context`.
//!
//! Targets the pieces left open elsewhere: the full `LocalTypeMap` API,
//! `Context`'s response helpers (`status`/`send`/`send_bytes`/`text`/`html`/
//! `json`/`redirect`), the `ConnectionFd` attribute, and the
//! `get_tcp_router` / `get_udp_router` lookups added by the codec-agnostic
//! router refactor.

use std::any::TypeId;
use std::net::SocketAddr;
use std::sync::Arc;

use aex::connection::context::{
    ConcurrentTypeMap, ConnectionFd, Context, LocalTypeMap, TcpRouterKey, TypeMapExt, UdpRouterKey,
    get_tcp_router, get_udp_router,
};
use aex::connection::global::GlobalContext;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::media_type::SubMediaType;
use aex::http::protocol::status::StatusCode;

fn make_ctx() -> Context {
    let addr: SocketAddr = "127.0.0.1:7000".parse().unwrap();
    let global = Arc::new(GlobalContext::new(addr, None));
    Context::new(None, None, global, addr)
}

#[test]
fn local_type_map_full_api() {
    let mut map = LocalTypeMap::new();
    assert_eq!(map.get_value::<i32>(), None);
    assert!(map.get_ref::<i32>().is_none());
    assert!(map.get_mut::<i32>().is_none());

    map.set_value(10i32);
    assert_eq!(map.get_value::<i32>(), Some(10));
    assert_eq!(map.get_ref::<i32>(), Some(&10));
    *map.get_mut::<i32>().unwrap() += 5;
    assert_eq!(map.get_value::<i32>(), Some(15));

    // Overwriting the same type keeps a single entry.
    map.set_value(99i32);
    assert_eq!(map.get_value::<i32>(), Some(99));

    map.clear();
    assert_eq!(map.get_value::<i32>(), None);
}

#[test]
fn context_new_defaults_and_accessors() {
    let ctx = make_ctx();
    assert_eq!(ctx.addr, "127.0.0.1:7000".parse().unwrap());
    assert!(ctx.local_addr.is_none());
    assert!(ctx.reader.is_none());
    assert!(ctx.writer.is_none());
    assert!(ctx.accepted.timestamp() > 0);
}

#[tokio::test]
async fn context_elapsed_grows() {
    let ctx = make_ctx();
    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    assert!(ctx.elapsed() >= 1);
}

#[test]
fn status_send_and_redirect_helpers() {
    let mut ctx = make_ctx();
    ctx.local.set_value(HttpMetadata::new());

    ctx.status(StatusCode::Created);
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(meta.status, StatusCode::Created);
    }

    ctx.text("plain");
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(meta.body.as_slice(), b"plain");
        assert_eq!(
            meta.headers
                .get(&HeaderKey::ContentType)
                .map(String::as_str),
            Some(SubMediaType::Plain.full_str())
        );
    }

    ctx.json("{\"ok\":true}");
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(meta.body.as_slice(), b"{\"ok\":true}");
        assert_eq!(
            meta.headers
                .get(&HeaderKey::ContentType)
                .map(String::as_str),
            Some(SubMediaType::Json.full_str())
        );
    }

    ctx.html("<h1>hi</h1>");
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(
            meta.headers
                .get(&HeaderKey::ContentType)
                .map(String::as_str),
            Some(SubMediaType::Html.full_str())
        );
    }

    ctx.send_bytes(vec![1, 2, 3], None);
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(meta.body, vec![1, 2, 3]);
        assert_eq!(
            meta.headers
                .get(&HeaderKey::ContentType)
                .map(String::as_str),
            Some(SubMediaType::OctetStream.full_str())
        );
    }

    ctx.redirect("/elsewhere");
    {
        let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
        assert_eq!(meta.status, StatusCode::Found);
        assert_eq!(
            meta.headers.get(&HeaderKey::Location).map(String::as_str),
            Some("/elsewhere")
        );
        assert!(meta.body.is_empty());
    }
}

#[test]
fn response_helpers_are_noops_without_http_metadata() {
    // No HttpMetadata in `local`: every helper must silently do nothing.
    let mut ctx = make_ctx();
    ctx.status(StatusCode::NotFound);
    ctx.text("ignored");
    ctx.send_bytes(vec![9], Some(SubMediaType::Json));
    ctx.redirect("/nowhere");
    assert!(ctx.local.get_ref::<HttpMetadata>().is_none());
}

#[test]
fn connection_fd_is_copy_and_debug() {
    let fd = ConnectionFd(3);
    let copied = fd;
    assert_eq!(copied.0, 3);
    assert!(format!("{:?}", fd).contains("ConnectionFd"));
}

#[test]
fn router_lookups_resolve_by_key_type() {
    let map = ConcurrentTypeMap::default();
    assert!(get_tcp_router(&map).is_none());
    assert!(get_udp_router(&map).is_none());

    let tcp = Arc::new(aex::tcp::router::Router::new());
    map.insert(TypeId::of::<TcpRouterKey>(), Box::new(tcp.clone()));
    let fetched = get_tcp_router(&map).expect("tcp router stored under its key type");
    assert!(Arc::ptr_eq(&fetched, &tcp));

    let udp = Arc::new(aex::udp::router::Router::new());
    map.insert(TypeId::of::<UdpRouterKey>(), Box::new(udp.clone()));
    let fetched = get_udp_router(&map).expect("udp router stored under its key type");
    assert!(Arc::ptr_eq(&fetched, &udp));

    // A value stored under a different type must not be returned.
    let other = ConcurrentTypeMap::default();
    other.set_value(Arc::new(aex::tcp::router::Router::new()));
    assert!(get_tcp_router(&other).is_none());
}
