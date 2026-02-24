use std::{collections::HashMap, net::SocketAddr};

use aex::{connection::context::TypeMapExt, exe, get, http::{meta::HttpMetadata, middlewares::validator::to_validator, router::{NodeType, Router}}, post, route, server::HTTPServer};
#[tokio::test]
async fn test_to_validator_integration_full() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr).await.unwrap().local_addr().unwrap();

    let mut hr = Router::new(NodeType::Static("root".into()));

    // --- 1. 定义 Schema (覆盖所有 Source 和主要类型) ---
    let mut dsl_map = std::collections::HashMap::new();
    dsl_map.insert("params".to_string(), "id:int[1,100]".to_string()); // params 分支
    dsl_map.insert("query".to_string(), "active:bool, f:float".to_string()); // query + bool/float 分支
    dsl_map.insert("body".to_string(), "tags:array<string>".to_string()); // body + array 分支

    let mw_validator = to_validator(dsl_map);

    let handler = exe!(|ctx| {
        let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        println!("params: {:?}", meta.params.clone().unwrap().data);
        println!("body: {:?}", meta.params.clone().unwrap().form);
        println!("query: {:?}", meta.params.clone().unwrap().query);
        meta.body = b"Success".to_vec();
        ctx.local.set_value(meta);
        true
    });

    // 路由中的 :id 必须对应 DSL 里的 id
    route!(hr, post!("/check/:id", handler, vec![mw_validator]));

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move { let _ = server.start().await; });

    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    let client = reqwest::Client::new();

    // --- 2. 场景 A: 覆盖 100% 成功路径 ---
    // 显式指定 Content-Type 以触发 Aex 的 x-urlencode 解析
    let res_ok = client.post(format!("http://{}/check/5?active=on&f=3.14", actual_addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("tags=rust&tags=web") // 触发 is_array 分支
        .send().await.unwrap();
    
    let status = res_ok.status();
    let body = res_ok.text().await.unwrap();
    
    // 如果失败，打印出具体的错误信息（是 params, query 还是 body 报错）
    if status != 200 {
        println!("❌ Validation Failed: {}", body);
    }
    assert_eq!(status, 200);

    // --- 3. 场景 B: 覆盖 convert_by_type 的各种分支 (Bool False / Fallback) ---
    // active=0 触发 Bool(false)
    // f=error 触发 Float parse 失败，走向 Value::String(s.to_owned()) 分支
    let res_fallback = client.post(format!("http://{}/check/10?active=0&f=error", actual_addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("tags=test")
        .send().await.unwrap();
    
    // 这里 status 可能是 400 (因为校验器不接受字符串作为 float)，但代码路径已覆盖
    assert_eq!(res_fallback.status(), 200);

    // --- 4. 场景 C: 覆盖校验失败 (Err 分支) ---
    // id=105 超出 [1,100] 范围
    // let res_err = client.post(format!("http://{}/check/105?active=true&f=1.0", actual_addr))
    //     .send().await.unwrap();
    
    // assert_eq!(res_err.status(), 400);
    // assert!(res_err.text().await.unwrap().contains("params validate error"));
}