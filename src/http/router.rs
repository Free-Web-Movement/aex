use std::{collections::HashMap, sync::Arc};

use crate::{
    connection::context::{HTTPContext, TypeMapExt},
    http::{meta::HttpMetadata, params::Params, protocol::{media_type::SubMediaType, status::StatusCode}, types::Executor},
};

/// 节点类型
#[derive(Clone, Debug)]
pub enum NodeType {
    Static(String), // 静态段
    Param(String),  // 动态段 :id
    Wildcard,       // 通配符 *
}

/// Trie 树节点
pub struct Router {
    pub node_type: NodeType,
    pub children: HashMap<String, Router>,
    pub middlewares: Option<HashMap<String, Vec<Arc<Executor>>>>, // 方法级中间件
    pub handlers: Option<HashMap<String, Arc<Executor>>>,         // 方法级处理器
}

// pub type Router = Router;

impl Router {
    pub fn new(node_type: NodeType) -> Self {
        Self {
            node_type,
            children: HashMap::new(),
            middlewares: None,
            handlers: None,
        }
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
            && let matched @ Some(_) = child.match_route(rest, params) {
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
}

// --------------------------------------
// 执行路由
// --------------------------------------

pub async fn handle_request(root: &Router, ctx: &mut HTTPContext) -> bool {
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
    if let Some(node) = root.match_route(&segments, &mut path_params) {
        // 4. 构造并填充 Params
        let mut params = Params::new(meta.path.clone());

        // Trie 树已经帮我们解析好了 data (Path Params)
        // 只有在 HashMap 不为空时才注入，保持数据清洁
        if !path_params.is_empty() {
            params.data = Some(path_params);
        }

        // 5. 处理 Form Body (如果是 x-www-form-urlencoded)
        if meta
            .content_type
            .to_string()
            .contains(SubMediaType::UrlEncoded.as_str())
            && meta.length > 0
        {
            let mut body_bytes = vec![0u8; meta.length];
            // 注意：这里直接从 ctx.reader 读取，因为 HTTPContext 暴露了 reader
            if tokio::io::AsyncReadExt::read_exact(&mut ctx.reader, &mut body_bytes)
                .await
                .is_ok()
            {
                params.set_form(&String::from_utf8_lossy(&body_bytes));
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
