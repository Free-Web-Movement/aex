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
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::Context;
use crate::http::meta::HttpMetadata;
use crate::http::params::{Params, SmallParams};
use crate::http::protocol::header::HeaderKey;

use crate::http::protocol::method::HttpMethod;
use crate::http::protocol::status::StatusCode;
use crate::http::protocol::version::HttpVersion;
use crate::http::types::{Executor, HandlerOutput, IntoExecutor};

#[derive(Debug, Clone)]
pub enum NodeType {
    Static(String),
    Param(String),
    Wildcard,
}

impl Default for NodeType {
    fn default() -> Self {
        NodeType::Static(String::new())
    }
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
    registered: bool,
}

impl<'a> RouteBuilder<'a> {
    fn new(
        router: &'a mut Router,
        method: &'static str,
        path: String,
        handler: Arc<Executor>,
        middlewares: Vec<Arc<Executor>>,
    ) -> Self {
        Self {
            router,
            method,
            path,
            handler,
            middlewares,
            registered: false,
        }
    }

    /// Add middleware to the route. Middlewares are executed before the handler.
    /// Accepts a plain closure or a pre-built `Arc<Executor>`.
    pub fn middleware<F>(mut self, mw: F) -> Self
    where
        F: IntoExecutor,
    {
        self.middlewares.push(mw.into_executor());
        self
    }

    fn do_register(&mut self) {
        if self.registered {
            return;
        }
        let segments: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();

        let method_key = self.method.to_uppercase();

        if segments.is_empty() {
            let router = &mut *self.router;
            router
                .handlers
                .get_or_insert_with(|| AHashMap::with_capacity(8))
                .insert(method_key.clone(), self.handler.clone());
            if !self.middlewares.is_empty() {
                router
                    .middlewares
                    .get_or_insert_with(|| AHashMap::with_capacity(4))
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

        current
            .handlers
            .get_or_insert_with(|| AHashMap::with_capacity(8))
            .insert(method_key.clone(), self.handler.clone());

        if !self.middlewares.is_empty() {
            current
                .middlewares
                .get_or_insert_with(|| AHashMap::with_capacity(4))
                .insert(method_key, self.middlewares.clone());
        }
    }
}

impl<'a> Drop for RouteBuilder<'a> {
    fn drop(&mut self) {
        self.do_register();
    }
}

/// Implemented by types whose methods declare routes via `#[aex::routes]`.
///
/// Generated automatically; mount an instance with `Router::push`.
#[doc(hidden)]
pub trait AexRoutes {
    fn __aex_register(router: &mut Router, this: Arc<Self>);
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

    /// Match a single segment against static or param children.
    /// Does NOT fall through to wildcard — wildcards are handled by match_route.
    #[inline]
    fn match_seg_exact<'a>(&'a self, seg: &str, params: &mut SmallParams) -> Option<&'a Router> {
        if let Some(node) = self.statics.get(seg) {
            return Some(node);
        }

        if let Some((ref name, ref node)) = self.param {
            if node.node_type.is_param() {
                params.insert(name.clone(), (*seg).to_string());
                return Some(node);
            }
        }

        None
    }

    fn register_handler<F, R>(
        &mut self,
        method: &'static str,
        path: &str,
        handler: F,
        middlewares: Vec<Arc<Executor>>,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        let executor: Arc<Executor> =
            Arc::new(move |ctx: &mut Context| Box::pin(handler(ctx).into_boxed(ctx)));
        RouteBuilder::new(self, method, path.to_string(), executor, middlewares)
    }

    fn register_with<F, R>(
        &mut self,
        method: &'static str,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler(method, path, handler, middlewares.into())
    }

    /// Fluent route registration: GET method.
    /// Registers automatically when the builder goes out of scope, so
    /// `router.get("/", |_| "Hello")`, `router.get("/", |ctx| { ... })` and the
    /// chained `router.get("/", handler).middleware(mw)` forms all work.
    pub fn get<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("GET", path, handler, Vec::new())
    }

    /// Fluent route registration: GET method with middlewares.
    /// Middlewares run before the handler; they sit between path and handler:
    /// `router.get_with("/admin", [auth, logger], |_| "Admin")`.
    pub fn get_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("GET", path, middlewares, handler)
    }

    /// Fluent route registration: POST method.
    pub fn post<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("POST", path, handler, Vec::new())
    }

    /// Fluent route registration: POST method with middlewares.
    pub fn post_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("POST", path, middlewares, handler)
    }

    /// Fluent route registration: PUT method.
    pub fn put<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("PUT", path, handler, Vec::new())
    }

    /// Fluent route registration: PUT method with middlewares.
    pub fn put_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("PUT", path, middlewares, handler)
    }

    /// Fluent route registration: DELETE method.
    pub fn delete<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("DELETE", path, handler, Vec::new())
    }

    /// Fluent route registration: DELETE method with middlewares.
    pub fn delete_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("DELETE", path, middlewares, handler)
    }

    /// Fluent route registration: PATCH method.
    pub fn patch<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("PATCH", path, handler, Vec::new())
    }

    /// Fluent route registration: PATCH method with middlewares.
    pub fn patch_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("PATCH", path, middlewares, handler)
    }

    /// Fluent route registration: OPTIONS method.
    pub fn options<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("OPTIONS", path, handler, Vec::new())
    }

    /// Fluent route registration: OPTIONS method with middlewares.
    pub fn options_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("OPTIONS", path, middlewares, handler)
    }

    /// Fluent route registration: HEAD method.
    pub fn head<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("HEAD", path, handler, Vec::new())
    }

    /// Fluent route registration: HEAD method with middlewares.
    pub fn head_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("HEAD", path, middlewares, handler)
    }

    /// Fluent route registration: matches all HTTP methods.
    pub fn all<F, R>(&mut self, path: &str, handler: F) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_handler("*", path, handler, Vec::new())
    }

    /// Fluent route registration: all HTTP methods with middlewares.
    pub fn all_with<F, R>(
        &mut self,
        path: &str,
        middlewares: impl Into<Vec<Arc<Executor>>>,
        handler: F,
    ) -> RouteBuilder<'_>
    where
        F: for<'a> Fn(&'a mut Context) -> R + Send + Sync + 'static,
        R: HandlerOutput + Send + 'static,
    {
        self.register_with("*", path, middlewares, handler)
    }

    /// Mount a class instance whose `&self` methods declare routes via
    /// `#[aex::routes]`.
    ///
    /// Registers every declared route (and its middlewares) in one call. The
    /// instance is captured, so handlers can operate on its state:
    ///
    /// ```rust,ignore
    /// #[aex::routes]
    /// impl Class {
    ///     // [auth] 是裸标识符，解析为同一实例上的对象中间件（&self 方法）
    ///     #[get(["/", "/"], [auth])]
    ///     fn index(&self, ctx: &mut Context) { ctx.text(&self.name); }
    ///
    ///     fn auth(&self, ctx: &mut Context) -> bool { /* 用 self 鉴权 */ true }
    /// }
    /// let mut router = Router::default();
    /// let instance = Class { name: "aex".into() };
    /// router.push(instance);
    /// ```
    pub fn push<C: AexRoutes + Send + Sync + 'static>(&mut self, instance: C) {
        C::__aex_register(self, Arc::new(instance));
    }

    /// Register a static file service. Files under `dir` are served at the URL
    /// prefix `prefix` with automatic MIME detection:
    ///
    /// ```rust,ignore
    /// router.static_files("/static", "public");
    /// // GET /static/app.js     -> public/app.js
    /// // GET /static            -> public/index.html
    /// // GET /static/sub/       -> public/sub/index.html
    /// ```
    ///
    /// Basic website resources (html/css/js/images/...) are supported out of
    /// the box. Files larger than 100 MiB are rejected with 404 — large files
    /// should be served by a dedicated download service. Use
    /// [`Router::static_files_with`] to customize the size limit / index file.
    pub fn static_files(&mut self, prefix: &str, dir: impl Into<PathBuf>) {
        self.static_files_with(prefix, crate::http::static_files::StaticFiles::new(dir));
    }

    /// Register a static file service with custom [`StaticFiles`] settings.
    ///
    /// [`StaticFiles`]: crate::http::static_files::StaticFiles
    pub fn static_files_with(
        &mut self,
        prefix: &str,
        config: crate::http::static_files::StaticFiles,
    ) {
        let prefix = prefix.trim();
        let exact = if prefix.starts_with('/') {
            prefix.to_string()
        } else {
            format!("/{prefix}")
        };
        let handler = config.build();
        // 通配路由覆盖前缀下的所有路径；精确路由让前缀本身也落到入口文件。
        self.insert(&format!("{exact}/*"), Some("GET"), handler.clone(), None);
        self.insert(&exact, Some("GET"), handler, None);
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
        node.handlers
            .get_or_insert_with(|| AHashMap::with_capacity(8))
            .insert(method_key.clone(), handler);

        if let Some(mws) = middlewares {
            node.middlewares
                .get_or_insert_with(|| AHashMap::with_capacity(4))
                .insert(method_key, mws);
        }
    }

    /// Match path segments against the route trie with wildcard backtracking.
    ///
    /// Priority:
    ///   1. Exact match (static/param) — continue to next segment
    ///   2. Current node's wildcard — matches remaining segments
    ///   3. Backtrack to ancestor wildcard — fallback when a deeper branch fails
    ///
    /// When a wildcard matches, the remaining segments (joined with `/`) are
    /// stored under the `"*"` param, so handlers can recover the rest of the
    /// path (e.g. for static file serving).
    #[inline]
    pub fn match_route<'a>(
        &'a self,
        segs: &[&str],
        params: &mut SmallParams,
    ) -> Option<&'a Router> {
        let mut current = self;
        let mut backtrack: Option<(usize, &'a Router)> = None;

        for (i, seg) in segs.iter().enumerate() {
            // Before descending into exact children, save this node's wildcard
            // as a backtrack target in case the branch fails deeper.
            if backtrack.is_none() {
                if let Some(n) = current.wildcard.as_ref().map(|n| n.as_ref()) {
                    backtrack = Some((i, n));
                }
            }

            // 1. Try exact match (static or param)
            if let Some(next) = current.match_seg_exact(seg, params) {
                current = next;
                continue;
            }

            // 2. No exact match — if this node has a wildcard, it matches the rest
            if let Some(wildcard) = &current.wildcard {
                Self::capture_wildcard(segs, i, params);
                return Some(wildcard.as_ref());
            }

            // 3. No wildcard here either — backtrack to ancestor wildcard
            if let Some((wi, wnode)) = backtrack {
                Self::capture_wildcard(segs, wi, params);
                return Some(wnode);
            }

            return None;
        }

        Some(current)
    }

    /// 剩余段匹配通配符时，捕获 `segs[start..]`（以 `/` 拼接）为 `"*"` 参数。
    fn capture_wildcard(segs: &[&str], start: usize, params: &mut SmallParams) {
        let rest = segs[start..].join("/");
        if !rest.is_empty() {
            params.insert("*".to_string(), rest);
        }
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
        let meta = match ctx.local.get_ref::<HttpMetadata>() {
            Some(meta) => meta,
            None => {
                tracing::error!("HttpMetadata missing in on_request");
                return false;
            }
        };

        let pure_path = meta
            .path
            .split_once('?')
            .map(|(p, _)| p)
            .unwrap_or(&meta.path);

        let segments: Vec<&str> = pure_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut path_params = SmallParams::with_capacity(segments.len().min(8));

        if let Some(node) = self.match_route(&segments, &mut path_params) {
            let path_full = meta.path.clone();
            let method = meta.method;
            let is_form = meta.content_type.is_form_urlencoded();
            let length = meta
                .headers
                .get(&HeaderKey::ContentLength)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);

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

            match ctx.local.get_mut::<HttpMetadata>() {
                Some(meta) => meta.params = Some(params),
                None => {
                    tracing::error!("HttpMetadata missing when setting params");
                    return false;
                }
            }

            let method_key = method.to_str();

            // 7. 执行中间件 (Middleware)
            if let Some(mws_map) = &node.middlewares {
                let mws = mws_map.get(method_key).or_else(|| mws_map.get("*"));
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
                    .get(method_key)
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

            ctx.local.clear();
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
        Router::new(NodeType::default())
    }
}
