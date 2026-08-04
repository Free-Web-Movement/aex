use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aex::connection::context::Context;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::router::Router;
use aex::http::types::Executor;
use aex::server::Server;
use tokio::time::sleep;

static EXEC_ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn mid(label: &'static str) -> Arc<Executor> {
    Arc::new(move |_ctx: &mut Context| {
        Box::pin(async move {
            EXEC_ORDER.lock().unwrap().push(label);
            true
        })
    })
}

struct User {
    name: String,
    api_key: String,
    created: Arc<Mutex<Vec<String>>>,
}

#[aex::routes]
impl User {
    // 多路径 + 中间件，&self 方法使用实例状态
    #[get(["/", "/profile"], [mid("A"), mid("B")])]
    fn profile(&self, ctx: &mut Context) {
        ctx.text(&self.name);
    }

    // 直接把 self.name 作为 String 返回
    #[get("/name")]
    fn name(&self, _ctx: &mut Context) -> String {
        self.name.clone()
    }

    // 单个路径、无中间件、async handler，使用实例状态
    #[post("/resources")]
    async fn create(&self, ctx: &mut Context) {
        self.created.lock().unwrap().push(self.name.clone());
        ctx.text("created");
    }

    // 返回 bool 的 async handler（无 self）
    #[get("/health")]
    async fn health(ctx: &mut Context) -> bool {
        ctx.text("healthy");
        true
    }

    // 对象中间件（同步）：裸标识符 -> &self 方法，读取实例的 api_key
    #[get("/secret", [auth])]
    fn secret(&self, ctx: &mut Context) {
        ctx.text("secret-ok");
    }

    // 对象中间件（异步）：同样能读 self
    #[get("/secure", [auth, audit])]
    fn secure(&self, ctx: &mut Context) {
        ctx.text("secure-ok");
    }

    fn auth(&self, ctx: &mut Context) -> bool {
        let key = HeaderKey::from_str("x-api-key").unwrap();
        ctx.get::<HttpMetadata>()
            .map(|m| m.headers.get(&key).is_some_and(|v| v == &self.api_key))
            .unwrap_or(false)
    }

    async fn audit(&self, ctx: &mut Context) -> bool {
        let key = HeaderKey::from_str("x-api-key").unwrap();
        ctx.get::<HttpMetadata>()
            .map(|m| m.headers.get(&key).is_some_and(|v| v == &self.api_key))
            .unwrap_or(false)
    }

    // 普通方法不会被当作路由
    fn helper() -> u32 {
        42
    }
}

struct Admin;

#[aex::routes]
impl Admin {
    #[get("/admin")]
    fn panel(ctx: &mut Context) -> &'static str {
        "admin"
    }
}

#[test]
fn test_routes_attribute_registers_all_paths() {
    let mut router = Router::default();
    router.push(User {
        name: "profile".into(),
        api_key: "secret".into(),
        created: Arc::new(Mutex::new(Vec::new())),
    });
    router.push(Admin);

    // 多路径
    assert!(router.has_route("GET", "/"));
    assert!(router.has_route("GET", "/profile"));
    // async 路由
    assert!(router.has_route("POST", "/resources"));
    assert!(router.has_route("GET", "/health"));
    // 对象中间件路由
    assert!(router.has_route("GET", "/secret"));
    assert!(router.has_route("GET", "/secure"));
    // 另一个实例
    assert!(router.has_route("GET", "/admin"));
}

#[tokio::test]
async fn test_routes_attribute_mount_http() {
    EXEC_ORDER.lock().unwrap().clear();
    let mut router = Router::default();
    router.push(User {
        name: "profile".into(),
        api_key: "secret".into(),
        created: Arc::new(Mutex::new(Vec::new())),
    });
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let server = Server::new(actual_addr, None);
    let server = server.http(router).clone();
    tokio::spawn(async move {
        let _ = server.start().await;
    });

    let client = reqwest::Client::new();

    // 多路径之一：/profile，验证中间件先于 handler 执行、handler 读到实例状态
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/profile", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "profile");
    assert_eq!(*EXEC_ORDER.lock().unwrap(), vec!["A", "B"]);

    // 多路径之二：/
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client.get(format!("http://{}/", actual_addr)).send().await {
            res = Some(r);
            break;
        }
    }
    assert_eq!(res.expect("server failed").text().await.unwrap(), "profile");

    // &self 方法直接把 self.name 作为 String 返回
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/name", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "profile");

    // async handler（POST），&self 方法写入实例状态
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .post(format!("http://{}/resources", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "created");

    // async handler 返回 bool
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/health", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.text().await.unwrap(), "healthy");

    // 对象中间件：无正确 header -> 拦截（400）
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secret", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // 对象中间件：带正确 header -> 放行，handler 执行
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secret", actual_addr))
            .header("x-api-key", "secret")
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "secret-ok");

    // async 对象中间件链 [auth, audit]
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secure", actual_addr))
            .header("x-api-key", "secret")
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "secure-ok");
}
