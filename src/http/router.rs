//! # HTTP Router
//!
//! Trie-tree based HTTP router supporting static, param, and wildcard paths.
//!
//! ## Path Types
//!
//! | Type | Example | Description |
//! |------|---------|-------------|
//! | Static | `/api/users` | Exact match |
//! | Param | `/api/users/:id` | Captures `:id` as parameter |
//! | Wildcard | `/static/*` | Matches any remaining path |

use ahash::AHashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::Context;
use crate::http::meta::HttpMetadata;
use crate::http::params::{Params, SmallParams};
use crate::http::protocol::header::HeaderKey;
use crate::http::protocol::media_type::SubMediaType;
use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::status::StatusCode;
use crate::http::protocol::version::HttpVersion;
use crate::http::types::Executor;

#[derive(Debug, Clone)]
pub enum NodeType {
    Static(String),
    Param(String),
    Wildcard,
}

impl NodeType {
    pub fn is_static(&self) -> bool {
        matches!(self, NodeType::Static(_))
    }
    pub fn is_param(&self) -> bool {
        matches!(self, NodeType::Param(_))
    }
    pub fn is_wildcard(&self) -> bool {
        matches!(self, NodeType::Wildcard)
    }
}

pub struct RouteBuilder<'a> {
    router: &'a mut Router,
    method: &'static str,
    path: String,
    handler: Arc<Executor>,
    middlewares: Vec<Arc<Executor>>,
}

impl<'a> RouteBuilder<'a> {
    fn new(
        router: &'a mut Router,
        method: &'static str,
        path: String,
        handler: Arc<Executor>,
    ) -> Self {
        Self {
            router,
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
        let segments: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();

        let method_key = self.method.to_uppercase();

        if segments.is_empty() {
            let router = &mut *self.router;
            if router.handlers.is_none() {
                router.handlers = Some(AHashMap::with_capacity(8));
            }
            router
                .handlers
                .as_mut()
                .unwrap()
                .insert(method_key.clone(), self.handler.clone());
            if !self.middlewares.is_empty() {
                if router.middlewares.is_none() {
                    router.middlewares = Some(AHashMap::with_capacity(4));
                }
                router
                    .middlewares
                    .as_mut()
                    .unwrap()
                    .insert(method_key, self.middlewares.clone());
            }
            return;
        }

        let mut current: &mut Router = self.router;
        for seg in &segments {
            current = if *seg == "*" {
                current
                    .wildcard
                    .get_or_insert_with(|| Box::new(Router::new(NodeType::Wildcard)))
            } else if seg.starts_with(':') {
                let (_, router) = current.param.get_or_insert_with(|| {
                    (
                        seg[1..].to_string(),
                        Box::new(Router::new(NodeType::Param(seg[1..].into()))),
                    )
                });
                &mut **router
            } else {
                current
                    .statics
                    .entry(seg.to_string())
                    .or_insert_with(|| Router::new(NodeType::Static(seg.to_string())))
            };
        }

        if current.handlers.is_none() {
            current.handlers = Some(AHashMap::with_capacity(8));
        }
        current
            .handlers
            .as_mut()
            .unwrap()
            .insert(method_key.clone(), self.handler.clone());

        if !self.middlewares.is_empty() {
            if current.middlewares.is_none() {
                current.middlewares = Some(AHashMap::with_capacity(4));
            }
            current
                .middlewares
                .as_mut()
                .unwrap()
                .insert(method_key, self.middlewares.clone());
        }
    }
}

/// Trie tree router for HTTP path matching.
pub struct Router {
    pub node_type: NodeType,
    pub statics: AHashMap<String, Router>,
    pub param: Option<(String, Box<Router>)>,
    pub wildcard: Option<Box<Router>>,
    pub middlewares: Option<AHashMap<String, Vec<Arc<Executor>>>>,
    pub handlers: Option<AHashMap<String, Arc<Executor>>>,
}

impl Router {
    /// Creates a new Router with the given node type.
    pub fn new(node_type: NodeType) -> Self {
        Self {
            node_type,
            statics: AHashMap::with_capacity(4),
            param: None,
            wildcard: None,
            middlewares: None,
            handlers: None,
        }
    }

    #[cfg(feature = "router-cache")]
    pub fn finalize(&mut self) {
        if let Some((_, ref mut child)) = self.param {
            child.finalize();
        }
        if let Some(ref mut child) = self.wildcard {
            child.finalize();
        }
        for (_, child) in &mut self.statics {
            child.finalize();
        }
    }

    #[inline]
    fn match_seg<'a>(&'a self, seg: &str, params: &mut SmallParams) -> Option<&'a Router> {
        // 1. Static match first
        if let Some(node) = self.statics.get(seg) {
            return Some(node);
        }

        // 2. Param match
        if let Some((ref name, ref node)) = self.param {
            if node.node_type.is_param() {
                params.insert(name.clone(), (*seg).to_string());
                return Some(node);
            }
        }

        // 3. Wildcard matches remaining path
        self.wildcard.as_ref().map(|n| n.as_ref())
    }

    /// Fluent route registration: GET method.
    pub fn get(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "GET", path.to_string(), handler)
    }

    /// Fluent route registration: POST method.
    pub fn post(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "POST", path.to_string(), handler)
    }

    /// Fluent route registration: PUT method.
    pub fn put(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "PUT", path.to_string(), handler)
    }

    /// Fluent route registration: DELETE method.
    pub fn delete(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "DELETE", path.to_string(), handler)
    }

    /// Fluent route registration: PATCH method.
    pub fn patch(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "PATCH", path.to_string(), handler)
    }

    /// Fluent route registration: OPTIONS method.
    pub fn options(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "OPTIONS", path.to_string(), handler)
    }

    /// Fluent route registration: HEAD method.
    pub fn head(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "HEAD", path.to_string(), handler)
    }

    /// Fluent route registration: matches all HTTP methods.
    pub fn all(&mut self, path: &str, handler: Arc<Executor>) -> RouteBuilder<'_> {
        RouteBuilder::new(self, "*", path.to_string(), handler)
    }

    /// Register a handler for a specific path and method.
    pub fn insert(
        &mut self,
        path: &str,
        method: Option<&str>,
        handler: Arc<Executor>,
        middlewares: Option<Vec<Arc<Executor>>>,
    ) {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let method_key = method.unwrap_or("*").to_uppercase();

        let mut current = self;
        for seg in &segments {
            current = if *seg == "*" {
                current
                    .wildcard
                    .get_or_insert_with(|| Box::new(Router::new(NodeType::Wildcard)))
            } else if seg.starts_with(':') {
                let (_, router) = current.param.get_or_insert_with(|| {
                    (
                        seg[1..].to_string(),
                        Box::new(Router::new(NodeType::Param(seg[1..].into()))),
                    )
                });
                &mut **router
            } else {
                current
                    .statics
                    .entry(seg.to_string())
                    .or_insert_with(|| Router::new(NodeType::Static(seg.to_string())))
            };
        }

        let node = current;
        if node.handlers.is_none() {
            node.handlers = Some(AHashMap::with_capacity(8));
        }
        node.handlers
            .as_mut()
            .unwrap()
            .insert(method_key.clone(), handler);

        // 设置中间件
        if let Some(mws) = middlewares {
            if node.middlewares.is_none() {
                node.middlewares = Some(AHashMap::with_capacity(4));
            }
            node.middlewares.as_mut().unwrap().insert(method_key, mws);
        }
    }

    /// 匹配路径（迭代版本，无回溯）
    #[inline]
    pub fn match_route<'a>(
        &'a self,
        segs: &[&str],
        params: &mut SmallParams,
    ) -> Option<&'a Router> {
        let mut current = self;
        for seg in segs {
            let next = current.match_seg(seg, params)?;
            if matches!(next.node_type, NodeType::Wildcard) {
                return Some(next);
            }
            current = next;
        }
        Some(current)
    }

    /// 从路由树中查找处理器（供 HTTP/2 使用）
    /// 返回: bool - 路由是否存在
    pub fn has_route(&self, method: &str, path: &str) -> bool {
        let pure_path = path.split('?').next().unwrap_or("");

        let segments: Vec<&str> = pure_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut params = crate::http::params::SmallParams::with_capacity(8.min(segments.len()));

        let node = match self.match_route(&segments, &mut params) {
            Some(n) => n,
            None => return false,
        };

        let method_key = method.to_uppercase();

        // 检查是否有 handler
        node.handlers
            .as_ref()
            .map(|h| h.contains_key(&method_key) || h.contains_key("*"))
            .unwrap_or(false)
    }

    // --------------------------------------
    // 执行路由
    // --------------------------------------

    pub async fn on_request(&self, ctx: &mut Context) -> bool {
        let pure_path = {
            let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
            meta.path.split('?').next().unwrap_or("").to_string()
        };

        let segments: Vec<&str> = pure_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut path_params = SmallParams::with_capacity(segments.len().min(8));

        if let Some(node) = self.match_route(&segments, &mut path_params) {
            let (path_full, method, is_form, length) = {
                let meta = ctx.local.get_ref::<HttpMetadata>().unwrap();
                let is_form = meta
                    .content_type
                    .to_string()
                    .contains(SubMediaType::UrlEncoded.as_str());
                let length = meta
                    .headers
                    .get(&crate::http::protocol::header::HeaderKey::ContentLength)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                (meta.path.clone(), meta.method, is_form, length)
            };
            let mut params = Params::new(path_full);

            if !path_params.is_empty() {
                params.data = Some(path_params.into());
            }

            if is_form && length > 0 {
                let mut body_bytes = vec![0u8; length];
                if let Some(r) = ctx.reader.as_deref_mut() {
                    let _ = r.read_exact(&mut body_bytes).await.is_ok();
                    params.set_form(&String::from_utf8_lossy(&body_bytes));
                } else {
                    return false;
                }
            }

            {
                let meta = ctx.local.get_mut::<HttpMetadata>().unwrap();
                meta.params = Some(params);
            }

            let method_key = method.to_str().to_uppercase();

            // 7. 执行中间件 (Middleware)
            if let Some(mws_map) = &node.middlewares {
                let mws = mws_map.get(&method_key).or_else(|| mws_map.get("*"));
                if let Some(mws) = mws {
                    for mw in mws {
                        if !mw(ctx).await {
                            if let Some(meta) = ctx.local.get_mut::<HttpMetadata>() {
                                if meta.status == StatusCode::Ok {
                                    meta.status = StatusCode::BadRequest;
                                }
                            }
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
            if let Some(meta) = ctx.local.get_mut::<HttpMetadata>() {
                meta.status = StatusCode::NotFound;
            }
        }
        true
    }

    /// Determine whether the connection should be kept alive after this request.
    fn wants_keep_alive(meta: &HttpMetadata) -> bool {
        match meta.version {
            HttpVersion::Http10 => meta
                .headers
                .get(&HeaderKey::Connection)
                .map(|v| v.eq_ignore_ascii_case("keep-alive"))
                .unwrap_or(false),
            HttpVersion::Http11 | HttpVersion::Http20 => !meta
                .headers
                .get(&HeaderKey::Connection)
                .map(|v| v.eq_ignore_ascii_case("close"))
                .unwrap_or(false),
        }
    }

    pub async fn handle(self: Arc<Self>, ctx: Arc<Mutex<Context>>) -> anyhow::Result<()> {
        loop {
            let guard = ctx.lock().await;
            let mut ctx = guard;

            if let Err(_) = ctx.req().parse_to_local().await {
                break;
            }

            let keep_alive = match ctx.local.get_ref::<HttpMetadata>() {
                Some(meta) => Self::wants_keep_alive(meta),
                None => false,
            };

            if self.on_request(&mut ctx).await {
                ctx.res().send_response().await?;
            } else {
                ctx.res().send_failure().await?;
            }

            if !keep_alive {
                break;
            }

            ctx.local = crate::connection::context::LocalTypeMap::new();
        }
        Ok(())
    }

    pub async fn is_http(self: Arc<Self>, ctx: Arc<Mutex<Context>>) -> anyhow::Result<bool> {
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
        }

        Ok(false)
    }
}

impl Default for Router {
    fn default() -> Self {
        Router::new(NodeType::Static("root".into()))
    }
}
