use std::{ collections::HashMap, sync::Arc };
use tokio::io::AsyncReadExt;

use crate::{ http::params::Params, http::types::{ Executor, HTTPContext } };
use crate::http::protocol::media_type::MediaType;

/// 节点类型
#[derive(Clone, Debug)]
pub enum NodeType {
    Static(String), // 静态段
    Param(String), // 动态段 :id
    Wildcard, // 通配符 *
}

/// Trie 树节点
pub struct Router {
    pub node_type: NodeType,
    pub children: HashMap<String, Router>,
    pub middlewares: Option<HashMap<String, Vec<Arc<Executor>>>>, // 方法级中间件
    pub handlers: Option<HashMap<String, Arc<Executor>>>, // 方法级处理器
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
                    Router::new(
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
    ) -> Option<&'a Router> {
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
pub async fn handle_request(root: &Router, ctx: &mut HTTPContext) -> bool {
    // 1️⃣ 直接将 path 按 '?' 分割成 [路径, 查询参数] 两部分
    let mut parts = ctx.req.path.splitn(2, '?');
    let pure_path = parts.next().unwrap_or("");
    let query_str = parts.next().unwrap_or("");

    // 2️⃣ 提取并更新 Query 参数 (确保 validator! 能在 query 字段拿到数据)
    if !query_str.is_empty() {
        ctx.req.params.query = Params::parse_pairs(query_str);
    }

    // 3️⃣ 🌟 特殊处理 Body：仅在 urlencoded 时解析
    // 检查 Content-Type 是否为 application/x-www-form-urlencoded
    if
        ctx.req.content_type.top_level == MediaType::Application &&
        ctx.req.content_type.sub_type.eq_ignore_ascii_case("x-www-form-urlencoded")
    {
        if !ctx.req.length > 0 {
            let length = ctx.req.length;
            let mut body = vec![0u8; length];
            if length > 0 {
                ctx.req.reader.read_exact(&mut body).await.unwrap_or_default();
                ctx.req.params.set_form(&String::from_utf8_lossy(&body));
            }
        }
    }

    // 3️⃣ 按纯路径切割 segments 用于 Trie 树匹配
    let segments: Vec<&str> = pure_path.trim_start_matches('/').split('/').collect();
    let mut params = HashMap::new();

    if let Some(node) = root.match_route(&segments, &mut params) {
        ctx.req.params.data = Some(params);

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
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use futures::FutureExt;
    use tokio::{
        io::{ AsyncReadExt, AsyncWriteExt, BufReader, BufWriter },
        net::{ TcpListener, TcpStream },
        sync::Mutex,
    };

    use crate::{
        exe,
        http::req::Request,
        http::res::Response,
        http::router::{ NodeType, Router, handle_request },
        http::types::{ HTTPContext, TypeMap, to_executor },
        v,
    };

    #[tokio::test]
    async fn test_http_server_get_route() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };

        // 1️⃣ 构建 Trie
        let mut root = Router::new(NodeType::Static("root".into()));

        root.insert(
            "/hello",
            Some("GET"),
            Arc::new(|ctx| {
                Box::pin(async move {
                    ctx.res.body.push("world".to_string());
                    true
                }).boxed()
            }),
            None
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("REASON"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            // 5️⃣ 写回响应
            // let resp_bytes = ctx.res.body.join("\r\n");
            ctx.res.send().await
            // Response::write_str(&mut ctx.res.writer, &resp_bytes).await
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
        let mut root = Router::new(NodeType::Static("root".into()));

        root.insert(
            "/user/:id",
            Some("POST"),
            Arc::new(|ctx| {
                Box::pin(async move {
                    ctx.res.body.push("posted".to_string());
                    true
                }).boxed()
            }),
            None
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("REASON"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;
            ctx.res.send().await

            // 5️⃣ 写回响应
            // let resp_bytes = ctx.res.body.join("\r\n");
            // Response::write_str(&mut ctx.res.writer, &resp_bytes).await
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
        let mut root = Router::new(NodeType::Static("root".into()));

        // POST 路由，不带 middleware
        crate::route!(
            root,
            crate::post!("/user/:id/profile", |ctx: &mut HTTPContext| {
                Box::pin(async move {
                    ctx.res.body.push("macro".to_string());
                    true
                }).boxed()
            })
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("REASON"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            ctx.res.send().await
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

    #[tokio::test]
    async fn test_http_server_get_route3() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };
        // use crate::make_method_macro;
        // 1️⃣ 构建 Trie
        let mut root = Router::new(NodeType::Static("root".into()));

        // POST 路由，不带 middleware
        crate::route!(
            root,
            crate::post!("/", |ctx: &mut HTTPContext| {
                Box::pin(async move {
                    ctx.res.body.push("root".to_string());
                    true
                }).boxed()
            })
        );

        // 2️⃣ 起 TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);

            let writer = BufWriter::new(writer);

            // 4️⃣ 生成 Request 对象
            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("Not a valid HTTP request!"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };
            // 4️⃣ 走 Trie
            handle_request(&root, &mut ctx).await;

            ctx.res.send().await
        });

        // 6️⃣ 客户端发请求
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        // 7️⃣ 断言
        assert!(resp_str.contains("root"));
    }

    #[tokio::test]
    async fn test_http_server_with_middlewares() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };

        let mut root = Router::new(NodeType::Static("root".into()));

        let mw1 = to_executor(|ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("mw1".to_string());
                true
            })
        });

        let mw2 = to_executor(|ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("mw2".to_string());
                true
            })
        });

        let handler = to_executor(|ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("handler".to_string());
                true
            })
        });

        root.insert("/mw", Some("GET"), handler, Some(vec![mw1, mw2]));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("REASON"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            handle_request(&root, &mut ctx).await;
            ctx.res.send().await
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /mw HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("mw1"));
        assert!(resp_str.contains("mw2"));
        assert!(resp_str.contains("handler"));
    }

    #[tokio::test]
    async fn test_http_server_middleware_break() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt };

        let mut root = Router::new(NodeType::Static("root".into()));

        let mw_block = to_executor(|ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("blocked".to_string());
                false // 中断
            })
        });

        let handler = to_executor(|ctx: &mut HTTPContext| {
            Box::pin(async move {
                ctx.res.body.push("handler".to_string());
                true
            })
        });

        root.insert("/stop", Some("GET"), handler, Some(vec![mw_block]));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            let req = Request::new(reader, peer_addr, "").await;
            let res = Response::new(writer);
            let mut ctx = HTTPContext {
                req: req.expect("REASON"),
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            handle_request(&root, &mut ctx).await;
            ctx.res.send().await
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /stop HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("blocked"));
        assert!(!resp_str.contains("handler"));
    }
    #[tokio::test]
    async fn test_route_with_exe_macro_and_pre() {
        use tokio::net::{ TcpListener, TcpStream };
        use tokio::io::{ AsyncReadExt, AsyncWriteExt, BufReader, BufWriter };
        use tokio::sync::Mutex;
        use std::sync::Arc;
        use crate::http::types::{ HTTPContext, TypeMap };
        use crate::http::req::Request;
        use crate::http::res::Response;
        use crate::http::router::{ Router, NodeType, handle_request };

        // ----------------------
        // 1️⃣ 构建 Router
        // ----------------------
        let mut root = Router::new(NodeType::Static("root".into()));

        // middleware 使用 exe! + pre
        let middleware = exe!(
            |ctx, data| {
                // body 捕获 pre 返回值 `data`
                ctx.res.body.push(format!("{}-mw", data));
                true
            },
            |ctx| {
                // pre 在 Box 外执行
                ctx.res.body.push("pre".to_string());
                // 返回给 body 使用
                "data".to_string()
            }
        );

        // handler
        let handler = exe!(|ctx| {
            ctx.res.body.push("handler".to_string());
            true
        });

        root.insert("/test", Some("GET"), handler, Some(vec![middleware]));

        // ----------------------
        // 2️⃣ 启动 TCP server
        // ----------------------
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let writer = BufWriter::new(writer);

            let req = Request::new(reader, peer_addr, "").await.unwrap();
            let res = Response::new(writer);

            let mut ctx = HTTPContext {
                req,
                res,
                global: Arc::new(Mutex::new(TypeMap::new())),
                local: TypeMap::new(),
            };

            handle_request(&root, &mut ctx).await;
            let _ = ctx.res.send().await;
        });

        // ----------------------
        // 3️⃣ 客户端请求
        // ----------------------
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /test HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        // ----------------------
        // 4️⃣ 断言
        // ----------------------
        // 预期执行顺序：pre -> body -> handler
        assert!(resp_str.contains("pre"));
        assert!(resp_str.contains("data-mw"));
        assert!(resp_str.contains("handler"));
    }

    #[tokio::test]
    async fn test_route_with_validator_macro() {
        // ... 前面的 import 保持不变 ...

        // ----------------------
        // 1️⃣ 构建路由
        // ----------------------
        let mut root = Router::new(NodeType::Static("root".into()));

        // ----------------------
        // 2️⃣ 构建 validator! 中间件
        // 修改点：DSL 字符串前后添加了 ()，这是你 Parser 的预期格式
        // ----------------------
        let middleware =
            v! {
        params => "(id:int[1,100])",
        body   => "(name:string[3,20])",
        query  => "(active?:bool)"
    };

        let handler = exe!(|ctx| {
            ctx.res.body.push("handler".to_string());
            true
        });

        root.insert("/create/:id", Some("POST"), handler, Some(vec![middleware]));

        // ----------------------
        // 3️⃣ 测试逻辑（以第一个合法请求为例）
        // ----------------------
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                let (reader, writer) = stream.into_split();
                let buf_reader = BufReader::new(reader);

                // 注意：Request::new 需要从 stream 读取，这里直接传入 reader
                let req = Request::new(buf_reader, peer_addr, "").await.unwrap();
                let res = Response::new(BufWriter::new(writer));

                let mut ctx = HTTPContext {
                    req,
                    res,
                    global: Arc::new(Mutex::new(TypeMap::new())),
                    local: TypeMap::new(),
                };

                handle_request(&root, &mut ctx).await;
                let _ = ctx.res.send().await;
            }
        });

        // ----------------------
        // 4️⃣ 客户端请求
        // ----------------------
        let mut client = TcpStream::connect(addr).await.unwrap();
        // 确保 Content-Length: 9 对应 "name=Eric"
        let req_bytes =
            b"POST /create/10?active=true HTTP/1.1\r\n\
                      Host: x\r\n\
                      Content-Type: application/x-www-form-urlencoded\r\n\
                      Content-Length: 9\r\n\r\n\
                      name=Eric";
        client.write_all(req_bytes).await.unwrap();

        let mut resp = vec![0; 1024];
        let n = client.read(&mut resp).await.unwrap();
        let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

        assert!(resp_str.contains("200 OK"));
        assert!(resp_str.contains("handler"));

        // ----------------------
        // 5️⃣ 失败测试
        // ----------------------
        // 同样，在失败用例的 validator! 宏里也要加上 ()
        // 并且发送 Content-Length: 7 对应 "name=ab"
    }
}
