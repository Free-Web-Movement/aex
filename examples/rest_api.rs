use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::{body, exe, get, post, put, delete, route};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

static USERS: once_cell::sync::Lazy<Mutex<HashMap<String, serde_json::Value>>> = 
    once_cell::sync::Lazy::new(|| {
        Mutex::new(HashMap::from([
            ("1".to_string(), serde_json::json!({"id": "1", "name": "Alice", "email": "alice@example.com"})),
            ("2".to_string(), serde_json::json!({"id": "2", "name": "Bob", "email": "bob@example.com"})),
        ]))
    });

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let secs = now.as_secs();
    let nanos = now.subsec_nanos() as u64;
    format!(
        "{:x}-{:x}-4{:x}-{:x}-{:x}",
        secs,
        nanos,
        ((secs ^ nanos) & 0x0fff) | 0x4000,
        ((secs << 2) & 0x3fff) | 0x8000,
        secs ^ nanos
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    let auth: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let auth_header = meta.headers.get(&HeaderKey::Authorization);

        if auth_header.is_none() {
            body!(ctx, r#"{"error":"Unauthorized","message":"Missing Authorization header"}"#);
            return false;
        }

        let token = auth_header.unwrap();
        if !token.starts_with("Bearer ") {
            body!(ctx, r#"{"error":"Unauthorized","message":"Invalid token format"}"#);
            return false;
        }

        ctx.local.set_value::<String>("user_123".to_string());
        true
    });

    let logger: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        println!("[{:?}] {} {}", meta.method, meta.path, ctx.addr);
        true
    });

    route!(router, get!(
        "/api/users",
        exe!(|ctx| {
            let users: Vec<_> = USERS.lock().unwrap().values().cloned().collect();
            body!(ctx, serde_json::to_string(&users).unwrap());
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
                .unwrap_or("");

            let users = USERS.lock().unwrap();
            if let Some(user) = users.get(id) {
                body!(ctx, serde_json::to_string(user).unwrap());
            } else {
                body!(ctx, r#"{"error":"Not Found","message":"User not found"}"#);
            }
            true
        }),
        vec![auth.clone(), logger.clone()]
    ));

    let create_handler: Arc<Executor> = exe!(move |ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let body_str = String::from_utf8_lossy(&meta.body);
        
        let user: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(u) => u,
            Err(_) => {
                body!(ctx, r#"{"error":"Bad Request","message":"Invalid JSON"}"#);
                return false;
            }
        };

        let id = user.get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid_v4());

        let user_with_id = serde_json::json!({
            "id": id,
            "name": user.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "email": user.get("email").and_then(|v| v.as_str()).unwrap_or(""),
        });

        USERS.lock().unwrap().insert(
            user_with_id["id"].as_str().unwrap().to_string(), 
            user_with_id.clone()
        );
        
        let response = serde_json::json!({
            "status": "created",
            "data": user_with_id
        });
        body!(ctx, serde_json::to_string(&response).unwrap());
        true
    });

    route!(router, post!(
        "/api/users",
        create_handler,
        vec![auth.clone(), logger.clone()]
    ));

    route!(router, put!(
        "/api/users/:id",
        exe!(|ctx| {
            let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            let id = meta.params
                .as_ref()
                .and_then(|p| p.data.as_ref())
                .and_then(|d| d.get("id"))
                .map(|v| v.as_str())
                .unwrap_or("");

            let body_str = String::from_utf8_lossy(&meta.body);
            let update: serde_json::Value = match serde_json::from_str(&body_str) {
                Ok(u) => u,
                Err(_) => {
                    body!(ctx, r#"{"error":"Bad Request"}"#);
                    return false;
                }
            };

            let mut users = USERS.lock().unwrap();
            if let Some(user) = users.get_mut(id) {
                if let Some(name) = update.get("name") {
                    user["name"] = name.clone();
                }
                if let Some(email) = update.get("email") {
                    user["email"] = email.clone();
                }
                body!(ctx, serde_json::to_string(user).unwrap());
            } else {
                body!(ctx, r#"{"error":"Not Found"}"#);
            }
            true
        }),
        vec![auth.clone(), logger.clone()]
    ));

    route!(router, delete!(
        "/api/users/:id",
        exe!(|ctx| {
            let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            let id = meta.params
                .as_ref()
                .and_then(|p| p.data.as_ref())
                .and_then(|d| d.get("id"))
                .map(|v| v.as_str())
                .unwrap_or("");

            let mut users = USERS.lock().unwrap();
            if users.remove(id).is_some() {
                body!(ctx, r#"{"status":"deleted"}"#);
            } else {
                body!(ctx, r#"{"error":"Not Found"}"#);
            }
            true
        }),
        vec![auth.clone(), logger.clone()]
    ));

    route!(router, get!(
        "/health",
        exe!(|ctx| {
            body!(ctx, r#"{"status":"healthy"}"#);
            true
        })
    ));

    println!("REST API Server running at http://{}", addr);
    println!("\nEndpoints:");
    println!("  GET    /health           - Health check (no auth)");
    println!("  GET    /api/users        - List users (no auth)");
    println!("  GET    /api/users/:id    - Get user by ID (auth required)");
    println!("  POST   /api/users        - Create user (auth required)");
    println!("  PUT    /api/users/:id    - Update user (auth required)");
    println!("  DELETE /api/users/:id    - Delete user (auth required)");
    println!("\nExample:");
    println!("  curl http://{}/api/users", addr);

    HTTPServer::new(addr, None)
        .http(router)
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
