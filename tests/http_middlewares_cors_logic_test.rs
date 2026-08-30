use aex::{
    connection::{context::Context, global::GlobalContext},
    http::{
        meta::HttpMetadata,
        middlewares::cors::CorsConfig,
        protocol::{header::HeaderKey, method::HttpMethod, status::StatusCode},
    },
};
use std::{net::SocketAddr, sync::Arc};

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

#[tokio::test]
async fn cors_default_writes_star_origin_and_full_header_set() {
    let mut ctx = make_ctx(9000);
    let ok = CorsConfig::new().build()(&mut ctx).await;
    assert!(ok);

    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowOrigin)
            .unwrap()
            .as_str(),
        "*"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowMethods)
            .unwrap()
            .as_str(),
        "GET, POST, PUT, DELETE, PATCH, OPTIONS"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowHeaders)
            .unwrap()
            .as_str(),
        "Content-Type, Authorization, X-Requested-With"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowCredentials)
            .unwrap()
            .as_str(),
        "true"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlMaxAge)
            .unwrap()
            .as_str(),
        "86400"
    );
}

#[tokio::test]
async fn cors_echoes_origin_header_when_present() {
    let mut ctx = make_ctx(9001);
    ctx.local
        .get_mut::<HttpMetadata>()
        .unwrap()
        .headers
        .insert(HeaderKey::Origin, "https://example.com");
    let ok = CorsConfig::new().build()(&mut ctx).await;
    assert!(ok);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowOrigin)
            .unwrap()
            .as_str(),
        "https://example.com"
    );
}

#[tokio::test]
async fn cors_allow_origin_all_false_skips_origin_header() {
    let mut ctx = make_ctx(9002);
    let ok = CorsConfig::new().allow_origin_all(false).build()(&mut ctx).await;
    assert!(ok);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert!(meta
        .headers
        .get(&HeaderKey::AccessControlAllowOrigin)
        .is_none());
}

#[tokio::test]
async fn cors_credentials_false_omits_credentials_header() {
    let mut ctx = make_ctx(9003);
    let ok = CorsConfig::new().allow_credentials(false).build()(&mut ctx).await;
    assert!(ok);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert!(meta
        .headers
        .get(&HeaderKey::AccessControlAllowCredentials)
        .is_none());
}

#[tokio::test]
async fn cors_options_short_circuits_and_returns_false() {
    let mut ctx = make_ctx(9004);
    ctx.local.get_mut::<HttpMetadata>().unwrap().method = HttpMethod::OPTIONS;
    let ok = CorsConfig::new().build()(&mut ctx).await;
    assert!(!ok);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(meta.status, StatusCode::Ok);
    assert!(meta.body.is_empty());
}

#[tokio::test]
async fn cors_methods_are_uppercased_and_custom_values_written() {
    let mut ctx = make_ctx(9005);
    let ok = CorsConfig::new()
        .allow_methods(vec!["get", "post"])
        .allow_headers(vec!["X-Custom"])
        .max_age(60)
        .build()(&mut ctx)
        .await;
    assert!(ok);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowMethods)
            .unwrap()
            .as_str(),
        "GET, POST"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowHeaders)
            .unwrap()
            .as_str(),
        "X-Custom"
    );
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlMaxAge)
            .unwrap()
            .as_str(),
        "60"
    );
}

#[tokio::test]
async fn cors_without_metadata_passes_through() {
    let a = addr(9006);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    assert!(CorsConfig::new().build()(&mut ctx).await);
}

#[tokio::test]
async fn cors_macro_builds_working_middleware() {
    let mut ctx = make_ctx(9007);
    assert!(aex::cors!()(&mut ctx).await);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert_eq!(
        meta.headers
            .get(&HeaderKey::AccessControlAllowOrigin)
            .unwrap()
            .as_str(),
        "*"
    );
}

#[tokio::test]
async fn cors_macro_with_chained_options() {
    let mut ctx = make_ctx(9008);
    assert!(aex::cors!(.allow_credentials(false))(&mut ctx).await);
    let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
    assert!(meta
        .headers
        .get(&HeaderKey::AccessControlAllowCredentials)
        .is_none());
}
