use std::{ any::TypeId, collections::HashMap, sync::Arc };
use crate::handler::{ Executor, HTTPContext };

/// 节点类型
#[derive(Clone, Debug)]
pub enum NodeType {
    Static(String), // 静态段
    Param(String), // 动态段 :id
    Wildcard, // 通配符 *
}

/// Trie 树节点
pub struct TrieNode {
    pub node_type: NodeType,
    pub children: HashMap<String, TrieNode>,
    pub middlewares: Option<HashMap<String, Vec<Arc<Executor>>>>, // 方法级中间件
    pub handlers: Option<HashMap<String, Arc<Executor>>>, // 方法级处理器
}

impl TrieNode {
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
        middlewares: Option<Vec<Arc<Executor>>>
    ) {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let mut node = self;

        for seg in segments {
            let key = if seg == "*" {
                "*".to_string()
            } else if seg.starts_with(':') {
                ":".to_string()
            } else {
                seg.to_string()
            };

            node = node.children
                .entry(key.clone())
                .or_insert_with(|| {
                    TrieNode::new(
                        if key == "*" {
                            NodeType::Wildcard
                        } else if key == ":" {
                            NodeType::Param(seg[1..].to_string())
                        } else {
                            NodeType::Static(seg.to_string())
                        }
                    )
                });
        }

        let method_key = method.map(|m| m.to_uppercase()).unwrap_or_else(|| "*".to_string());

        // 设置处理器
        if node.handlers.is_none() {
            node.handlers = Some(HashMap::new());
        }
        node.handlers.as_mut().unwrap().insert(method_key.clone(), handler);

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
        params: &mut HashMap<String, String>
    ) -> Option<&'a TrieNode> {
        if segs.is_empty() {
            return Some(self);
        }

        let seg = segs[0];
        let rest = &segs[1..];

        // 1. 静态匹配
        if let Some(child) = self.children.get(seg) {
            if let matched @ Some(_) = child.match_route(rest, params) {
                return matched;
            }
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
pub async fn handle_request(root: &TrieNode, ctx: &mut HTTPContext<'_>) -> bool {
    let segments: Vec<&str> = ctx.req.path.trim_start_matches('/').split('/').collect();
    let mut params = HashMap::new();

    if let Some(node) = root.match_route(&segments, &mut params) {
        ctx.local.insert(TypeId::of::<HashMap<String, String>>(), Box::new(params));

        let method_key = ctx.req.method.to_str();

        // 执行中间件
        if let Some(mws_map) = &node.middlewares {
            let mws = mws_map.get(method_key).or_else(|| mws_map.get("*"));
            if let Some(mws) = mws {
                for mw in mws {
                    let cont = mw(ctx).await;
                    if !cont {
                        // (mw.fallback)(ctx).await;
                        return false;
                    }
                }
            }
        }

        // 执行处理器
        if let Some(handlers_map) = &node.handlers {
            let handler = handlers_map.get(method_key).or_else(|| handlers_map.get("*"));
            if let Some(handler) = handler {
                return handler(ctx).await;
            } else {
                println!("405 Method Not Allowed: {}", ctx.req.method.to_str());
            }
        } else {
            println!("404 Not Found: {}", ctx.req.path);
        }
    } else {
        println!("404 Not Found: {}", ctx.req.path);
    }

    false
}

#[cfg(test)]
mod tests {
    use std::{ collections::HashMap, sync::Arc };
    use futures::FutureExt;
    use tokio::io::{ BufReader, BufWriter };

    use crate::{
        handler::HTTPContext,
        req::Request,
        res::Response,
        route,
        trie::{ NodeType, TrieNode, handle_request },
    };

    #[tokio::test]
    async fn test_http_server_get_route() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };

        // 1️⃣ 构建 Trie
        let mut root = TrieNode::new(NodeType::Static("root".into()));

        root.insert(
            "/hello",
            Some("GET"),
            Arc::new(|ctx|
                (
                    async move {
                        ctx.res.body.push("world");
                        true
                    }
                ).boxed()
            ),
            None
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let mut writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(&mut writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: HashMap::new(),
                local: HashMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            // 5️⃣ 写回响应
            let resp_bytes = ctx.res.body.join("\r\n");
            Response::<'_>::write_str(&mut writer, &resp_bytes).await
        });

        // 6️⃣ 客户端发请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /hello HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        // 7️⃣ 断言
        assert!(resp_str.contains("world"));
    }

    #[tokio::test]
    async fn test_http_server_get_route1() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };

        // 1️⃣ 构建 Trie
        let mut root = TrieNode::new(NodeType::Static("root".into()));

        root.insert(
            "/user/:id",
            Some("POST"),
            Arc::new(|ctx|
                (
                    async move {
                        // let data = ctx.req.params.data.as_ref().unwrap().get("id").unwrap().as_str();

                        // println!("id = {}", data);
                        // ctx.res.body.push(data.clone().to_string().as_str());
                        ctx.res.body.push("posted");
                        true
                    }
                ).boxed()
            ),
            None
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let mut writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(&mut writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: HashMap::new(),
                local: HashMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            // 5️⃣ 写回响应
            let resp_bytes = ctx.res.body.join("\r\n");
            Response::<'_>::write_str(&mut writer, &resp_bytes).await
        });

        // 6️⃣ 客户端发请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"POST /user/ddidi HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        // 7️⃣ 断言
        assert!(resp_str.contains("posted"));
    }

    #[tokio::test]
    async fn test_http_server_get_route2() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };
        // use crate::make_method_macro;
        // 1️⃣ 构建 Trie
        let mut root = TrieNode::new(NodeType::Static("root".into()));

        // POST 路由，不带 middleware
        crate::route!(
            root,
            crate::post!("/user/:id/profile", |ctx: &mut HTTPContext|
                (
                    async move {
                        println!("POST Handler");
                        ctx.res.body.push("macro");
                        true
                    }
                ).boxed()
            )
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let mut writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(&mut writer);
            let mut ctx = HTTPContext {
                req,
                res,
                global: HashMap::new(),
                local: HashMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            // 5️⃣ 写回响应
            let resp_bytes = ctx.res.body.join("\r\n");
            Response::<'_>::write_str(&mut writer, &resp_bytes).await
        });

        // 6️⃣ 客户端发请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"POST /user/ddidi/profile HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        // 7️⃣ 断言
        assert!(resp_str.contains("macro"));
    }
}
