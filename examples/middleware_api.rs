use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::{exe, get, post, route};
use std::net::SocketAddr;
use std::sync::Arc;

fn auth_middleware() -> Arc<Executor> {
    exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let auth_header = meta.headers.get(&HeaderKey::Authorization);

        if auth_header.is_none() {
            ctx.send("Unauthorized: Missing Authorization header");
            return false;
        }

        let token = auth_header.unwrap();
        if !token.starts_with("Bearer ") {
            ctx.send("Unauthorized: Invalid token format");
            return false;
        }

        ctx.local.set_value::<String>("user_123".to_string());
        true
    })
}

fn logging_middleware() -> Arc<Executor> {
    exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        println!("[{:?}] {} {}", meta.method, meta.path, ctx.addr);
        true
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    let auth = auth_middleware();
    let logger = logging_middleware();

    route!(router, get!(
        "/api/users",
        exe!(|ctx| {
            ctx.send(r#"["user1", "user2", "user3"]"#);
            true
        }),
        vec![logger.clone()]
    ));

    route!(router, get!(
        "/api/users/:id",
        exe!(|ctx| {
            let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            let id = meta.params
                .as_ref()
                .and_then(|p| p.data.as_ref())
                .and_then(|d| d.get("id"))
                .map(|v| v.as_str())
                .unwrap_or("unknown");
            ctx.send(format!(r#"{{"id":"{}","name":"User {}"}}"#, id, id));
            true
        }),
        vec![auth.clone(), logger.clone()]
    ));

    route!(router, post!(
        "/api/users",
        exe!(|ctx| {
            let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            let body = String::from_utf8_lossy(&meta.body);
            println!("Create user: {}", body);
            ctx.send(format!(r#"{{"status":"created","data":{}}}"#, body));
            true
        }),
        vec![auth.clone(), logger.clone()]
    ));

    route!(router, get!(
        "/public/*",
        exe!(|ctx| {
            let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            ctx.send(format!(r#"{{"path":"{}"}}"#, meta.path));
            true
        })
    ));

    println!("Server running at http://{}", addr);
    println!("Try:");
    println!("  curl http://{}/public/info", addr);
    println!("  curl http://{}/api/users", addr);
    println!("  curl -H 'Authorization: Bearer token' http://{}/api/users/123", addr);

    HTTPServer::new(addr, None)
        .http(router)
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
