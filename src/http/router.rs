//! # HTTP Router
//!
//! Trie-tree based HTTP router supporting static, param, and wildcard paths.
//!
//! ## Path Types
//!
//! | Type | Example | Description |
//! |------|---------|-------------|
//! | Static | `/api/users` | Exact path match |
//! | Param | `/api/users/:id` | Captures URL segment |
//! | Wildcard | `/api/*` | Matches remaining path
//!
//! ## Fluent API (Recommended)
//!
//! ```rust,ignore
//! use aex::http::router::{NodeType, Router as HttpRouter};
//! use aex::exe;
//!
//! let mut router = HttpRouter::new(NodeType::Static("root".into()));
//!
//! router.get("/api/users", handler).register();
//! router.post("/api/users", create_handler).middleware(auth).register();
//! ```

use std::{collections::HashMap, sync::Arc};
use std::cell::RefCell;
use std::rc::Rc;
use std::result::Result::Ok;

use tokio::{io::AsyncReadExt, sync::Mutex};

use crate::{
    connection::context::{Context, TypeMapExt},
    http::{
        meta::HttpMetadata,
        params::Params,
        protocol::{media_type::SubMediaType, method::HttpMethod, status::StatusCode},
        types::Executor,
    },
};

/// Node type for Trie tree router.
#[derive(Clone, Debug)]
pub enum NodeType {
    /// Static path segment (e.g., "users")
    Static(String),
    /// Parameter segment (e.g., ":id" captures "123")
    Param(String),
    /// Wildcard segment (* matches all remaining)
    Wildcard,
}

/// Builder for fluent route registration.
///
/// # Example
///
/// ```rust,ignore
/// router.get("/api/users", handler)
///     .middleware(auth)
///     .middleware(logging)
///     .register();
/// ```
pub struct RouteBuilder<'a> {
    router: Rc<RefCell<&'a mut Router>>,
    method: &'static str,
    path: String,
    handler: Arc<Executor>,
    middlewares: Vec<Arc<Executor>>,
}

impl<'a> RouteBuilder<'a> {
    fn new(router: &'a mut Router, method: &'static str, path: String, handler: Arc<Executor>) -> Self {
        Self {
            router: Rc::new(RefCell::new(router)),
            method,
            path,
            handler,
            middlewares: Vec::new(),
        }
    }

    /// Add middleware to the route. Middlewares are executed before the handler.
    pub fn middleware(mut self, mw: Arc<Executor>) -> Self {
        self.middlewares.push(mw);
        self
    }

    /// Register the route with the router.
    pub fn register(self) {
        let segments: Vec<&str> = self.path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        
        let method_key = self.method.to_uppercase();
        
        {
            let mut router = self.router.borrow_mut();
            
            if segments.is_empty() {
                if router.handlers.is_none() {
                    router.handlers = Some(HashMap::new());
                }
                router.handlers.as_mut().unwrap().insert(method_key.clone(), self.handler.clone());
                if !self.middlewares.is_empty() {
                    if router.middlewares.is_none() {
                        router.middlewares = Some(HashMap::new());
                    }
                    router.middlewares.as_mut().unwrap().insert(method_key, self.middlewares.clone());
                }
                return;
            }

            let mut current: &mut Router = &mut *router;
            for seg in &segments {
                let key = if *seg == "*" {
                    "*".to_string()
                } else if seg.starts_with(':') {
                    ":".to_string()
                } else {
                    seg.to_string()
                };

                let entry = current.children.entry(key.clone()).or_insert_with(|| {
                    Router::new(if key == "*" {
                        NodeType::Wildcard
                    } else if key == ":" {
                        NodeType::Param(seg[1..].to_string())
                    } else {
                        NodeType::Static(seg.to_string())
                    })
                });
                current = entry;
            }

            if current.handlers.is_none() {
                current.handlers = Some(HashMap::new());
            }
            current.handlers.as_mut().unwrap().insert(method_key.clone(), self.handler.clone());

            if !self.middlewares.is_empty() {
                if current.middlewares.is_none() {
                    current.middlewares = Some(HashMap::new());
                }
                current.middlewares.as_mut().unwrap().insert(method_key, self.middlewares.clone());
            }
        }
    }
}

/// Trie tree router for HTTP path matching.
pub struct Router {
    pub node_type: NodeType,
    pub children: HashMap<String, Router>,
    pub middlewares: Option<HashMap<String, Vec<Arc<Executor>>>>,
    pub handlers: Option<HashMap<String, Arc<Executor>>>,
}

impl Router {
    /// Creates a new Router with the given node type.
    pub fn new(node_type: NodeType) -> Self {
        Self {
            node_type,
            children: HashMap::new(),
            middlewares: None,
            handlers: None,
        }
    }

    /// Fluent route registration: GET method.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// router.get("/api/users", handler)
    ///     .middleware(auth)
    ///     .register();
    /// ```
    pub fn get(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "GET", path.to_string(), handler)
    }

    /// Fluent route registration: POST method.
    pub fn post(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "POST", path.to_string(), handler)
    }

    /// Fluent route registration: PUT method.
    pub fn put(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "PUT", path.to_string(), handler)
    }

    /// Fluent route registration: DELETE method.
    pub fn delete(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "DELETE", path.to_string(), handler)
    }

    /// Fluent route registration: PATCH method.
    pub fn patch(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "PATCH", path.to_string(), handler)
    }

    /// Fluent route registration: OPTIONS method.
    pub fn options(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "OPTIONS", path.to_string(), handler)
    }

    /// Fluent route registration: HEAD method.
    pub fn head(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "HEAD", path.to_string(), handler)
    }

    /// Fluent route registration: matches all HTTP methods.
    pub fn all(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder {
        RouteBuilder::new(self, "*", path.to_string(), handler)
    }

    /// 插入路由
    pub fn insert(
        &mut self,
        path: &str,
        method: Option<&str>,
        handler: Arc<Executor>,
        middlewares: Option<Vec<Arc<Executor>>>,
    ) {
        // let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        let segments: Vec<&str> = path
            .split('/')
            .filter(|s| !s.is_empty()) // 保持一致，过滤掉空段
            .collect();
        let mut node = self;

        for seg in segments {
            let key = if seg == "*" {
                "*".to_string()
            } else if seg.starts_with(':') {
                ":".to_string()
            } else {
                seg.to_string()
            };

            node = node.children.entry(key.clone()).or_insert_with(|| {
                Router::new(if key == "*" {
                    NodeType::Wildcard
                } else if key == ":" {
                    NodeType::Param(seg[1..].to_string())
                } else {
                    NodeType::Static(seg.to_string())
                })
            });
        }

        let method_key = method
            .map(|m| m.to_uppercase())
            .unwrap_or_else(|| "*".to_string());

        // 设置处理器
        if node.handlers.is_none() {
            node.handlers = Some(HashMap::new());
        }
        node.handlers
            .as_mut()
            .unwrap()
            .insert(method_key.clone(), handler);

        // 设置中间件
        if let Some(mws) = middlewares {
            if node.middlewares.is_none() {
                node.middlewares = Some(HashMap::new());
            }
            node.middlewares.as_mut().unwrap().insert(method_key, mws);
        }
    }

    /// 匹配路径
    pub fn match_route<'a>(
        &'a self,
        segs: &[&str],
        params: &mut HashMap<String, String>,
    ) -> Option<&'a Router> {
        if segs.is_empty() {
            return Some(self);
        }

        let seg = segs[0];
        let rest = &segs[1..];

        // 1. 静态匹配
        if let Some(child) = self.children.get(seg)
            && let matched @ Some(_) = child.match_route(rest, params)
        {
            return matched;
        }

        // 2. 动态匹配
        if let Some(param_child) = self.children.get(":") {
            if let NodeType::Param(name) = &param_child.node_type {
                params.insert(name.clone(), seg.to_string());
            }
            if let matched @ Some(_) = param_child.match_route(rest, params) {
                return matched;
            }
        }

        // 3. 通配符匹配
        if let Some(wildcard_child) = self.children.get("*") {
            return Some(wildcard_child);
        }

        None
    }

    pub async fn handle(self: Arc<Self>, ctx: Arc<Mutex<Context>>) -> anyhow::Result<()> {
        // let reader: Option<BoxReader> = Some(Box::new(reader));
        // let writer: Option<BoxWriter> = Some(Box::new(writer));
        // let mut ctx = Context::new(reader, writer, global, peer_addr);

        let guard = ctx.lock().await;
        let mut ctx = guard;
        ctx.req().parse_to_local().await?;
        // handle_request 返回 true 表示所有中间件和 Handler 正常通过
        // 返回 false 表示被拦截（如 validator 发现类型不匹配）
        if self.on_request(&mut ctx).await {
            // 🟢 正常出口
            ctx.res().send_response().await?;
        } else {
            // 🔴 错误/拦截出口
            // 此时 send_failure 会读取 validator 写入的 "'{}' is not a valid boolean"
            ctx.res().send_failure().await?;
        }
        Ok(())
    }

    // --------------------------------------
    // 执行路由
    // --------------------------------------

    pub async fn on_request(&self, ctx: &mut Context) -> bool {
        // 1. 获取 Metadata
        let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap(); // 注意这里直接从 local 获取并可变借用

        // 2. 准备路由匹配所需的 segments
        let pure_path = meta.path.split('?').next().unwrap_or("");
        let segments: Vec<&str> = pure_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut path_params = HashMap::new();

        // 3. 执行 Trie 树匹配
        if let Some(node) = self.match_route(&segments, &mut path_params) {
            // 4. 构造并填充 Params
            let mut params = Params::new(meta.path.clone());

            // Trie 树已经帮我们解析好了 data (Path Params)
            // 只有在 HashMap 不为空时才注入，保持数据清洁
            if !path_params.is_empty() {
                params.data = Some(path_params);
            }

            // 5. 处理 Form Body (如果是 x-www-form-urlencoded)
            let length = match meta
                .headers
                .get(&super::protocol::header::HeaderKey::ContentLength)
            {
                Some(s) => {
                    let v = match s.parse::<usize>() {
                        Ok(u) => u,
                        Err(_) => 0,
                    };
                    v
                }
                None => 0,
            };
            if meta
                .content_type
                .to_string()
                .contains(SubMediaType::UrlEncoded.as_str())
                && length > 0
            {
                let mut body_bytes = vec![0u8; length];
                // 注意：这里直接从 ctx.reader 读取，因为 Context 暴露了 reader

                if let Some(r) = ctx.reader.as_deref_mut() {
                    let _ = r.read_exact(&mut body_bytes).await.is_ok();
                    params.set_form(&String::from_utf8_lossy(&body_bytes));
                } else {
                    return false;
                }
            }

            // 6. 关键步骤：更新 meta 并同步回 ctx.local
            meta.params = Some(params);
            ctx.local.set_value(meta.clone()); // 同步更新回 local，确保后续中间件和处理器能访问到最新的 Metadata

            // let method_key = meta.method.to_str().to_owned(); // 提前拷贝一份用于匹配
            let method_key = meta.method.to_str().to_uppercase(); // 强制大写以匹配 HashMap 的 Key

            // 7. 执行中间件 (Middleware)
            if let Some(mws_map) = &node.middlewares {
                let mws = mws_map.get(&method_key).or_else(|| mws_map.get("*"));
                if let Some(mws) = mws {
                    for mw in mws {
                        if !mw(ctx).await {
                            // 如果中间件没有设置状态，我们补一个默认的 400
                            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                            // meta.status = StatusCode::BadRequest;
                            if meta.status == StatusCode::Ok {
                                meta.status = StatusCode::BadRequest;
                            }
                            ctx.local.set_value(meta);
                            return false;
                        }
                    }
                }
            }

            // 8. 执行最终处理器 (Handler)
            if let Some(handlers_map) = &node.handlers {
                let handler = handlers_map
                    .get(&method_key)
                    .or_else(|| handlers_map.get("*"));
                if let Some(handler) = handler {
                    return handler(ctx).await;
                }
            }
        } else {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.status = StatusCode::NotFound;
            ctx.local.set_value(meta);
        }
        true
    }

    pub async fn is_http(self: Arc<Self>, ctx: Arc<Mutex<Context>>) -> anyhow::Result<bool> {
        // ⚡ 优化：临时取走 Reader 进行探测，避免在 I/O 等待时锁死 Context
        let reader = {
            let mut guard = ctx.lock().await;
            guard.reader.take()
        };

        if let Some(mut inner_reader) = reader {
            let is_http = HttpMethod::is_http_connection(&mut inner_reader).await?;
            
            // 将 Reader 放回 Context
            {
                let mut guard = ctx.lock().await;
                guard.reader = Some(inner_reader);
            }

            if is_http {
                self.handle(ctx).await?;
                return Ok(true);
            }
        } else {
            return Ok(false);
        }

        Ok(false)
    }
}

impl Default for Router {
    fn default() -> Self {
        Router::new(NodeType::Static("root".into()))
    }
}
