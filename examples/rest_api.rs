use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::Server;
use aex::exe;
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
            ctx.send(r#"{"error":"Unauthorized"}"#, None);
            return false;
        }
        true
    });

    let logger: Arc<Executor> = exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        println!("[{:?}] {} {}", meta.method, meta.path, ctx.addr);
        true
    });

    router.get("/api/users", exe!(|ctx| {
        let users: Vec<_> = USERS.lock().unwrap().values().cloned().collect();
        ctx.send(serde_json::to_string(&users).unwrap(), None);
        true
    })).middleware(logger.clone()).register();

    router.get("/api/users/:id", exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let id = meta.params
            .as_ref()
            .and_then(|p| p.data.as_ref())
            .and_then(|d| d.get("id"))
            .map(|v| v.as_str())
            .unwrap_or("");
        let users = USERS.lock().unwrap();
        if let Some(user) = users.get(id) {
            ctx.send(serde_json::to_string(user).unwrap(), None);
        } else {
            ctx.send(r#"{"error":"Not Found"}"#, None);
        }
        true
    })).middleware(auth.clone()).middleware(logger.clone()).register();

    router.post("/api/users", exe!(move |ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let body_str = String::from_utf8_lossy(&meta.body);
        let user: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(u) => u,
            Err(_) => {
                ctx.send(r#"{"error":"Bad Request"}"#, None);
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
        ctx.send(serde_json::to_string(&user_with_id).unwrap(), None);
        true
    })).middleware(auth.clone()).middleware(logger.clone()).register();

    router.put("/api/users/:id", exe!(|ctx| {
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
                ctx.send(r#"{"error":"Bad Request"}"#, None);
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
            ctx.send(serde_json::to_string(user).unwrap(), None);
        } else {
            ctx.send(r#"{"error":"Not Found"}"#, None);
        }
        true
    })).middleware(auth.clone()).middleware(logger.clone()).register();

    router.delete("/api/users/:id", exe!(|ctx| {
        let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        let id = meta.params
            .as_ref()
            .and_then(|p| p.data.as_ref())
            .and_then(|d| d.get("id"))
            .map(|v| v.as_str())
            .unwrap_or("");
        let mut users = USERS.lock().unwrap();
        if users.remove(id).is_some() {
            ctx.send(r#"{"status":"deleted"}"#, None);
        } else {
            ctx.send(r#"{"error":"Not Found"}"#, None);
        }
        true
    })).middleware(auth.clone()).middleware(logger.clone()).register();

    router.get("/health", exe!(|ctx| {
        ctx.send(r#"{"status":"healthy"}"#, None);
        true
    })).register();

    println!("REST API Server running at http://{}", addr);

    Server::new(addr, None)
        .http(router)
        .start()
        .await?;
    Ok(())
}
