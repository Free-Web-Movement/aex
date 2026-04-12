use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::{body, exe};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    let auth: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let auth_header = meta.headers.get(&HeaderKey::Authorization);

        if auth_header.is_none() {
            body!(ctx, r#"{"error":"Unauthorized"}"#);
            return false;
        }
        true
    });

    let logger: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        println!("[{:?}] {} {}", meta.method, meta.path, ctx.addr);
        true
    });

    let home: Arc<Executor> = exe!(|ctx| {
        body!(ctx, "Welcome to AEX!");
        true
    });

    let users: Arc<Executor> = exe!(|ctx| {
        body!(ctx, r#"["user1", "user2", "user3"]"#);
        true
    });

    let user_detail: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let id = meta.params
            .as_ref()
            .and_then(|p| p.data.as_ref())
            .and_then(|d| d.get("id"))
            .map(|v| v.as_str())
            .unwrap_or("unknown");
        body!(ctx, format!(r#"{{"id":"{}"}}"#, id));
        true
    });

    let health: Arc<Executor> = exe!(|ctx| {
        body!(ctx, r#"{"status":"healthy"}"#);
        true
    });

    router.get("/", home).register();

    router.get("/api/users", users)
        .middleware(logger.clone())
        .register();

    router.get("/api/users/:id", user_detail)
        .middleware(auth.clone())
        .middleware(logger.clone())
        .register();

    router.get("/health", health).register();

    println!("Fluent API Server running at http://{}", addr);
    println!("\nEndpoints:");
    println!("  GET /              - Home page");
    println!("  GET /api/users     - List users (with logging)");
    println!("  GET /api/users/:id - User detail (auth + logging)");
    println!("  GET /health        - Health check");

    HTTPServer::new(addr, None)
        .http(router)
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
