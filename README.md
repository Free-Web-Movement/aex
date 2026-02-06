AEX.rs - Rust 异步 HTTP + WebSocket 框架

简介
----
AEX.rs 是一个轻量级异步 Rust Web 框架，提供 HTTP 路由、中间件和 WebSocket 支持。
框架以 cargo aex 发布，核心功能包括：
- HTTP 路由处理
- 中间件链
- WebSocket 升级与消息处理
- 异步执行，基于 Tokio Runtime

核心概念
--------
1. Router：Trie 树路由，支持静态和动态路径匹配
2. Handler：异步处理 HTTP 请求，返回 bool 表示是否继续中间件链
3. Middleware：异步中间件，可组合多个功能
4. WebSocket：提供握手、发送/接收消息、Ping/Pong、关闭连接
5. Executor：统一封装 Handler 和 Middleware 类型

宏使用
--------
框架提供路由生成宏，快速创建 Handler + 可选 Middleware

HTTP 方法宏：
- get!(path, handler [, middleware])
- post!(path, handler [, middleware])
- put!, delete!, patch!, options!, head!, trace!, connect!
- all!(path, handler [, middleware]) 匹配所有方法

示例：
let get_home = get!("/home", |ctx: &mut HTTPContext| {
    Box::pin(async move {
        ctx.res.body.push("Welcome!".to_string());
        true
    })
});

可选中间件：
let ws_mw = WebSocket::to_middleware(ws);
let get_ws = get!("/ws", |ctx: &mut HTTPContext| {
    Box::pin(async { true })
}, vec![ws_mw]);

route! 宏（注册路由）：
route!(root, get_home);
route!(root, get_ws);

说明：
- 如果方法为 "*"，表示匹配所有 HTTP 方法
- Handler 与 Middleware 自动封装为 Arc<Executor>
- 支持多个中间件组合

WebSocket 中间件
-----------------
使用 WebSocket::to_middleware(ws) 生成中间件，可用于路由注册。
示例：
let ws = WebSocket {
    on_text: Some(Arc::new(|_ws, _ctx, msg| Box::pin(async move {
        println!("收到文本: {}", msg);
        true
    }))),
    on_binary: Some(Arc::new(|_ws, _ctx, data| Box::pin(async move {
        println!("收到二进制数据: {:?}", data);
        true
    }))),
};

let ws_mw = WebSocket::to_middleware(ws);

let get_ws = get!("/ws", |ctx: &mut HTTPContext| Box::pin(async { true }), vec![ws_mw]);
route!(root, get_ws);

访问 /ws 即可完成 WebSocket 升级和消息处理

框架优势
---------
- 高度异步，适配 Tokio
- 简洁宏设计，快速生成路由和中间件
- 完整 WebSocket 支持（握手、消息、Ping/Pong、关闭）
- Middleware 链条灵活，可组合 WebSocket、认证、日志等
- Router 支持静态、动态路径匹配及全方法路由

目录结构
---------
src/
  aex.rs         # 框架核心
  router.rs      # Router + Trie 节点
  websocket.rs   # WebSocket 核心功能
  types.rs       # HTTPContext, Executor, Handler 类型
  macros.rs      # get!, post!, route! 等宏

示例启动
-----------
#[tokio::main]
async fn main() {
    let mut root = Router::new(NodeType::Static("root".into()));

    let ws = WebSocket {
        on_text: Some(Arc::new(|_ws, _ctx, msg| Box::pin(async move {
            println!("{}", msg);
            true
        }))),
        on_binary: None,
    };

    let ws_mw = WebSocket::to_middleware(ws);

    let get_ws = get!("/ws", |ctx: &mut HTTPContext| Box::pin(async { true }), vec![ws_mw]);
    route!(root, get_ws);

    AexServer::new(root).bind("127.0.0.1:8080").await.unwrap().run().await;
}

说明：
- 路由 /ws 支持 WebSocket 升级
- 中间件链可组合多个功能
- 支持 HTTP 普通路由与 WebSocket 混合注册
