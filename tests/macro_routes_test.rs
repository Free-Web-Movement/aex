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

// 全局函数中间件（仅同步；async 需 _async! 包装）
fn global_auth(ctx: &mut Context) -> bool {
    let key = HeaderKey::from_str("x-api-key").unwrap();
    ctx.get::<HttpMetadata>()
        .map(|m| m.headers.get(&key).is_some_and(|v| v == "secret"))
        .unwrap_or(false)
}

// 名为 no_prefix 的全局中间件：必须用数组 [no_prefix] 包裹才会被当作中间件
fn no_prefix(ctx: &mut Context) -> bool {
    let key = HeaderKey::from_str("x-np").unwrap();
    ctx.get::<HttpMetadata>()
        .map(|m| m.headers.contains_key(&key))
        .unwrap_or(false)
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

    // 裸标识符 [auth] → 级联：先找 &self 方法 auth（找到，走实例方法）
    #[get("/secret", [auth])]
    fn secret(&self, ctx: &mut Context) {
        ctx.text("secret-ok");
    }

    // self.auth → 显式实例方法
    #[get("/secret2", [self.auth])]
    fn secret2(&self, ctx: &mut Context) {
        ctx.text("secret2-ok");
    }

    // [self.auth, audit] → 混排：self.auth（显式实例）+ audit（裸标识符→找self方法）
    #[get("/secure", [self.auth, audit])]
    fn secure(&self, ctx: &mut Context) {
        ctx.text("secure-ok");
    }

    // 裸标识符 [global_auth] → 级联：不在 impl → 全局函数（sync）
    #[get("/public", [global_auth])]
    fn public(&self, ctx: &mut Context) {
        ctx.text("public-ok");
    }

    // 裸标识符 [check] → 级联：找关联函数（None receiver）check
    #[get("/checked", [check])]
    fn checked(&self, ctx: &mut Context) {
        ctx.text("checked-ok");
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

    // 关联函数（无 self）：裸标识符 `check` 级联到 step 2
    fn check(ctx: &mut Context) -> bool {
        let key = HeaderKey::from_str("x-check").unwrap();
        ctx.get::<HttpMetadata>()
            .map(|m| m.headers.contains_key(&key))
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

struct Api;

#[aex::routes(prefix = "/backend")]
impl Api {
    #[get("/login")]
    fn login(ctx: &mut Context) -> &'static str {
        "login"
    }

    #[post("/api/orders")]
    fn orders(ctx: &mut Context) -> &'static str {
        "orders"
    }

    // no_prefix：注册为 /ping 而非 /backend/ping
    #[get("/ping", no_prefix)]
    fn ping(ctx: &mut Context) -> &'static str {
        "ping"
    }

    // [no_prefix] 在数组内 → 是中间件（全局函数 no_prefix），不是标记；前缀仍生效 → /backend/np-mid
    #[get("/np-mid", [no_prefix])]
    fn np_mid(ctx: &mut Context) -> &'static str {
        "np-mid-ok"
    }
}

struct Nested;

#[aex::routes("/backend/api")]
impl Nested {
    #[get("/users")]
    fn users(ctx: &mut Context) -> &'static str {
        "users"
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
    router.push(Api);
    router.push(Nested);

    // 多路径
    assert!(router.has_route("GET", "/"));
    assert!(router.has_route("GET", "/profile"));
    // async 路由
    assert!(router.has_route("POST", "/resources"));
    assert!(router.has_route("GET", "/health"));
    // 对象中间件路由（bare ident）
    assert!(router.has_route("GET", "/secret"));
    // 显式 self.method 路由
    assert!(router.has_route("GET", "/secret2"));
    // 混排路由
    assert!(router.has_route("GET", "/secure"));
    // 全局函数中间件路由
    assert!(router.has_route("GET", "/public"));
    // 关联函数中间件路由
    assert!(router.has_route("GET", "/checked"));
    // 另一个实例
    assert!(router.has_route("GET", "/admin"));
    // prefix 路由
    assert!(router.has_route("GET", "/backend/login"));
    assert!(router.has_route("POST", "/backend/api/orders"));
    assert!(router.has_route("GET", "/backend/api/users"));
    // no_prefix 路由（跳过 prefix，注册在根路径）
    assert!(router.has_route("GET", "/ping"));
    assert!(!router.has_route("GET", "/backend/ping"));
    // [no_prefix] 数组内是中间件而非标记 → 前缀仍生效，注册在 /backend/np-mid
    assert!(router.has_route("GET", "/backend/np-mid"));
    assert!(!router.has_route("GET", "/np-mid"));
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
    router.push(Api);
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

    // 裸标识符 [auth]：无正确 header → 拦截（400）
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

    // 裸标识符 [auth]：带正确 header → 放行，handler 执行
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

    // self.auth（显式实例方法）：无 header → 400
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secret2", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // self.auth（显式实例方法）：带正确 header → 200
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secret2", actual_addr))
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
    assert_eq!(response.text().await.unwrap(), "secret2-ok");

    // 混排 [self.auth, audit]：无 header → 400
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/secure", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // 混排 [self.auth, audit]：带 header → 200
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

    // 全局函数 [global_auth]：无 header → 400
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/public", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // 全局函数 [global_auth]：带 header → 200
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/public", actual_addr))
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
    assert_eq!(response.text().await.unwrap(), "public-ok");

    // 关联函数 [check]：无 header → 400
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/checked", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // 关联函数 [check]：带 header → 200
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/checked", actual_addr))
            .header("x-check", "yes")
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "checked-ok");

    // [no_prefix] 数组内是中间件：无 x-np header → 400
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/backend/np-mid", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 400);

    // [no_prefix] 数组内是中间件：带 x-np header → 200
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/backend/np-mid", actual_addr))
            .header("x-np", "yes")
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "np-mid-ok");

    // no_prefix 路由注册在根路径：GET /ping 直接可访问
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/ping", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "ping");

    // no_prefix 路由：/backend/ping 不应存在
    let mut res = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(100)).await;
        if let Ok(r) = client
            .get(format!("http://{}/backend/ping", actual_addr))
            .send()
            .await
        {
            res = Some(r);
            break;
        }
    }
    let response = res.expect("Server failed to respond");
    assert_eq!(response.status().as_u16(), 404);
}
