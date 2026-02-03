use std::{ collections::HashMap, net::SocketAddr };
use regex::Regex;
use tokio::{ io::{ BufReader, BufWriter }, net::TcpStream };

use crate::{
    handler::{ HTTPContext, Handler },
    params::Params,
    protocol::{ method::HttpMethod, status::StatusCode },
    req::Request,
    res::Response,
};
use crate::handler::Executor;

use std:: sync::Arc;

macro_rules! http_methods {
    ($($fn_name:ident => $method:expr),+ $(,)?) => {
        $(
            #[inline]
            pub fn $fn_name(
                &mut self,
                paths: Vec<&str>,
                executors: Vec<Arc<Executor>>,
            ) -> &mut Self {
                self.add(paths, vec![$method], executors)
            }
        )+
    };
}

/// 路由条目
#[derive(Clone)]
pub struct RouteEntry {
    pub regex: Regex, // 匹配正则
    pub raw_path: String, // 原始路径
    pub handler: Handler, // 处理器
    pub param_names: Vec<String>, // 路径参数名
}

/// Router
#[derive(Clone)]
pub struct Router {
    pub routes: Vec<RouteEntry>,
    pub executors: Vec<Arc<Executor>>,
}

impl Router {
    http_methods! {
        get     => "GET",
        post    => "POST",
        put     => "PUT",
        delete  => "DELETE",
        patch   => "PATCH",
        options => "OPTIONS",
        head    => "HEAD",
        trace   => "TRACE",
        connect => "CONNECT",
    }

    /// 创建 Router
    pub fn new() -> Self {
        Self { routes: vec![], executors: vec![] }
    }

    /// 注册路由
    pub fn add(
        &mut self,
        paths: Vec<&str>,
        methods: Vec<&str>,
        executors: Vec<Arc<Executor>>
    ) -> &mut Self {
        for path in paths {
            let (regex_str, param_names) = Params::parse_path_regex(path);
            let re = Regex::new(&regex_str).unwrap();

            if let Some(entry) = self.routes.iter_mut().find(|r| r.raw_path == path) {
                for method in &methods {
                    let m = HttpMethod::from_str(method).unwrap();
                    entry.handler.add_vec(&mut param_names.clone(), Some(m), executors.clone());
                }
            } else {
                let mut handler = Handler::new();
                for method in &methods {
                    let m = HttpMethod::from_str(method).unwrap();
                    handler.add_vec(&mut param_names.clone(), Some(m), executors.clone());
                }
                self.routes.push(RouteEntry {
                    regex: re,
                    raw_path: path.to_string(),
                    handler,
                    param_names,
                });
            }
        }
        self
    }

    pub async fn process(&self, ctx: &mut HTTPContext<'_>) {
        // 先读取 path / method（只读，不跨 await）
        let (req_path, req_method) = {
            let req = &ctx.req;
            (req.path.clone(), req.method.clone())
        };
        for route in &self.routes {
            if let Some(caps) = route.regex.captures(&req_path) {
                // ---------- 填充 path 参数 ----------
                let mut path_params = HashMap::new();
                for (i, name) in route.param_names.iter().enumerate() {
                    if let Some(m) = caps.get(i + 1) {
                        path_params.insert(name.clone(), m.as_str().to_string());
                    }
                }

                {
                    let req = &mut ctx.req;
                    req.params.data = Some(path_params);
                }

                // ---------- 取 executors（必须 clone，不能跨 await 持 borrow） ----------

                let executors: Vec<Arc<Executor>> = route.handler
                    .get_executors(Some(&req_method))
                    .clone();

                // ---------- 串行执行 middleware ----------
                for exec in executors {
                    let continue_chain = exec(ctx).await;
                    if !continue_chain {
                        break;
                    }
                }

                return;
            }
        }
    }

    pub async fn on_request(&self, stream: TcpStream, peer_addr: SocketAddr) {
        let (reader, writer) = stream.into_split();
        // let reader = Arc::new(Mutex::new(reader));
        // let writer: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>> = Arc::new(Mutex::new(writer));
        let mut reader = BufReader::new(reader);

        let mut writer = BufWriter::new(writer);

        // 1️⃣ 先读取请求行 / URL
        // ⚠️ 这里假设 Request::parse_url 只解析 URL，不生成完整 Request
        let url = match Request::peek_url(&mut reader).await {
            Ok(u) => u,
            Err(_) => {
                // 无法读取 URL，直接返回 400
                let _ = Response::send_status(&mut writer, StatusCode::BadRequest, None).await;
                return;
            }
        };

        // 2️⃣ 匹配路由
        let mut matched_route: Option<&RouteEntry> = None;
        for route in &self.routes {
            let url_str = url.as_ref().unwrap();
            if route.regex.is_match(url_str) {
                matched_route = Some(route);
                break;
            }
        }

        if matched_route.is_none() {
            // 3️⃣ 未匹配到路由 → 返回 404
            let _ = Response::send_status(&mut writer, StatusCode::NotFound, None).await;
            return;
        }

        let route: &RouteEntry = matched_route.unwrap();

        // 4️⃣ 生成 Request 对象
        let req = Request::new(reader, peer_addr, &route.raw_path).await;
        let res = Response::new(&mut writer);

        let mut ctx = HTTPContext {
            req,
            res,
            global: HashMap::new(),
            local: HashMap::new(),
        };

        // 5️⃣ 执行全局 middleware
        for exec in &self.executors {
            if !exec(&mut ctx).await {
                return;
            }
        }

        // 6️⃣ 填充 path 参数

        let method;
        {
            let req = &mut ctx.req;
            let data = Params::extract_params(
                &url.unwrap().to_string(),
                &route.raw_path
            ).unwrap_or_default();

            method = req.method.clone();
            req.params.data = Some(data);
        }

        // 7️⃣ 执行路径相关 middleware
        let executors = route.handler.get_executors(Some(&method)).clone();
        for exec in executors {
            if !exec(&mut ctx).await {
                break;
            }
        }
    }
}
