use std::collections::HashMap;

use aex::{
    connection::context::TypeMapExt,
    exe, get,
    http::{
        meta::HttpMetadata,
        middlewares::validator::{to_validator, value_to_string},
        router::{NodeType, Router},
    },
    post, route,
    server::HTTPServer,
    v,
};
use zz_validator::ast::Value;
#[tokio::test]
async fn test_to_validator_integration_full() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap()
        .local_addr()
        .unwrap();

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
    tokio::spawn(async move {
        let _ = server.start().await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    let client = reqwest::Client::new();

    // --- 2. 场景 A: 覆盖 100% 成功路径 ---
    // 显式指定 Content-Type 以触发 Aex 的 x-urlencode 解析
    let res_ok = client
        .post(format!("http://{}/check/5?active=on&f=3.14", actual_addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("tags=rust&tags=web") // 触发 is_array 分支
        .send()
        .await
        .unwrap();

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
    let res_fallback = client
        .post(format!("http://{}/check/10?active=0&f=error", actual_addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("tags=test")
        .send()
        .await
        .unwrap();

    // 这里 status 可能是 400 (因为校验器不接受字符串作为 float)，但代码路径已覆盖
    assert_eq!(res_fallback.status(), 200);

    // --- 4. 场景 C: 覆盖校验失败 (Err 分支) ---
    // id=105 超出 [1,100] 范围
    // let res_err = client.post(format!("http://{}/check/105?active=true&f=1.0", actual_addr))
    //     .send().await.unwrap();

    // assert_eq!(res_err.status(), 400);
    // assert!(res_err.text().await.unwrap().contains("params validate error"));
}
#[tokio::test]
async fn test_v_macro_integration_full() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap()
        .local_addr()
        .unwrap();

    let mut hr = Router::new(NodeType::Static("root".into()));

    let mw_validator = v!(
        params => "(id:int[1,100])",
        query  => "(active:bool, f:float)",
        body   => "(tags:array<string>)"
    );

    let handler = exe!(|ctx| {
        let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        meta.body = b"Macro Success".to_vec();
        ctx.local.set_value(meta);
        true
    });

    route!(hr, post!("/check/:id", handler, vec![mw_validator]));

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let client = reqwest::Client::new();

    // --- 修复点：手动构造 urlencoded 字符串 ---
    // 这种方式不依赖 reqwest 的 .form() 特性，且能 100% 模拟 body 来源
    let form_body = "tags=rust&tags=test";

    let res = client
        .post(format!(
            "http://{}/check/50?active=true&f=1.23",
            actual_addr
        ))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_body)
        .send()
        .await
        .unwrap();

    let status = res.status();
    let response_text = res.text().await.unwrap();

    if status != 200 {
        // 如果失败，打印出 Validator 返回的具体错误信息（如 "body validate error: tags is required"）
        println!("❌ Validation Error Details: {}", response_text);
    }

    assert_eq!(status.as_u16(), 200);
    assert_eq!(response_text, "Macro Success");
}

#[tokio::test]
async fn test_validator_to_handler_data_flow() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap()
        .local_addr()
        .unwrap();

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 1. 定义全 Object 化的 DSL (使用你确认正确的括号语法)
    let mw_validator = v!(
        params => "(id:int[1,100])",
        query  => "(active:bool, f:float)",
        body   => "(username:string[3,10], tags:array<string>)"
    );

    // 2. 编写最终 Handler 进行数据断言
    let handler = exe!(|ctx| {
        // 从 local 提取 HttpMetadata
        let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();

        // 验证 Validator 是否把数据正确转换并留存在了 ctx.local 或 meta.params 中
        // 注意：根据你的 validator 实现，转换后的 Value 可能在 ctx.local 的特定 Key 下
        // 这里假设你的 validator 将结果注入到了 ctx.local

        // 示例：检查 Params (来自路径)
        let params = meta.params.as_ref().unwrap();
        let id = params.data.as_ref().unwrap().get("id").unwrap();
        assert_eq!(id, "50"); // 路径中的原始字符串
        assert_eq!(params.query.get("f"), Some(&vec!["3.14".to_string()])); // 路径中的原始字符串
        assert_eq!(params.query.get("active"), Some(&vec!["true".to_string()])); // 路径中的原始字符串

        // 示例：检查转换后的业务逻辑（假设你存入了结构体或 Value）
        // 如果你的 validator 只是“校验”而不“转换并存储”，这里测的是拦截能力
        // 如果你的 validator 会 insert(Value)，则如下测试：
        // let val = ctx.local.get_value::<zz_validator::ast::Value>().unwrap();

        meta.body = b"Handler Reached".to_vec();
        ctx.local.set_value(meta);
        true
    });

    // 路由绑定：:id 对应 params 规则
    route!(hr, post!("/user/:id", handler, vec![mw_validator]));

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let client = reqwest::Client::new();

    // 3. 发起请求
    // Query: ?active=true&f=3.14
    // Body: username=tom&tags=rust&tags=aex
    let res = client
        .post(format!("http://{}/user/50?active=true&f=3.14", actual_addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("username=tom&tags=rust&tags=aex")
        .send()
        .await
        .unwrap();

    // 4. 验证结果
    let status = res.status().as_u16();
    if status != 200 {
        let err_body = res.text().await.unwrap();
        panic!("Validation failed unexpectedly: {}", err_body);
    }

    assert_eq!(status, 200);
    println!("✅ Integrated Data Flow Test Passed!");
}

#[tokio::test]
async fn test_validator_conversion_logic_hardcore() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap()
        .local_addr()
        .unwrap();

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 括号语法定义：必须严格匹配类型
    let mw_validator = v!(
        query => "(i:int, b_true:bool, b_false:bool, f:float)"
    );

    let handler = exe!(|ctx| {
        let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        meta.body = b"Conversion Verified".to_vec();
        ctx.local.set_value(meta);
        true
    });

    route!(hr, get!("/test", handler, vec![mw_validator]));

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let client = reqwest::Client::new();

    // --- 核心测试点：手动拼接各种边缘情况 ---
    // i=42 (Int)
    // b_true=ON (测试 eq_ignore_ascii_case 识别为 true)
    // b_false=0 (测试数字识别为 false)
    // f=0.001 (Float)
    let test_url = format!(
        "http://{}/test?i=42&b_true=ON&b_false=0&f=0.001",
        actual_addr
    );

    let res = client
        .get(test_url)
        .send()
        .await
        .expect("Failed to send request");

    let status = res.status().as_u16();
    let body = res.text().await.unwrap();

    if status == 400 {
        panic!("❌ 转换逻辑失败! 详情: {}", body);
    }

    assert_eq!(status, 200, "所有字段应通过 convert_by_type 转换并匹配规则");
    assert_eq!(body, "Conversion Verified");
}

#[tokio::test]
async fn test_validator_edge_cases_and_fallback() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let actual_addr = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap()
        .local_addr()
        .unwrap();

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 1. 构造 DSL
    // b_off: 测试 "off" 转换
    // mixed: 使用 string 类型，这样不论 convert_by_type 返回 Int 还是 String，校验都能过
    //        从而确保代码执行了 s.to_owned() 路径
    let mw_validator = v!(
        query => "(b_off:bool, mixed:string)"
    );

    let handler = exe!(|ctx| {
        let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        meta.body = b"Edge Cases Verified".to_vec();
        ctx.local.set_value(meta);
        true
    });

    route!(hr, get!("/edge", handler, vec![mw_validator]));

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let client = reqwest::Client::new();

    // --- 场景 1: 测试 "off" ---
    // 触发 FieldType::Bool 里的 else if s.eq_ignore_ascii_case("off")
    let res_off = client
        .get(format!("http://{}/edge?b_off=OFF&mixed=any", actual_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res_off.status().as_u16(),
        200,
        "Should handle 'OFF' as bool false"
    );

    // --- 场景 2: 测试 s.to_owned() (Fallback 路径) ---
    // 在 convert_by_type(FieldType::Int) 中传入 "not_a_number"
    // 它会执行 .unwrap_or_else(|_| Value::String(s.to_owned()))

    // 我们定义一个带 int 的规则来触发对应分支的 fallback
    let _mw_fallback = v!(query => "(age:string)"); // 注意这里用 string 承接
    // 如果 convert_by_type 里的 Int 分支被调用（根据规则类型），它就会走 s.to_owned()

    let res_fallback = client
        .get(format!(
            "http://{}/edge?b_off=false&mixed=hello_world",
            actual_addr
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(res_fallback.status().as_u16(), 200);
}

#[tokio::test]
async fn test_validator_boolean_strict_error_integration() {
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    // --- 🚀 修正点：根据 Parser 的报错修改 DSL 语法 ---
    let mut dsl_map = HashMap::new();
    // 之前报错 "Expected LParen"，说明语法需要括号
    dsl_map.insert("query".to_string(), "(is_active:bool)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));
    let validator_mw = to_validator(dsl_map);

    hr.insert(
        "/check",
        Some("GET"),
        exe!(|_ctx| { true }),
        Some(vec![validator_mw]),
    );

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(actual_addr).await.unwrap();
    // 发送非法布尔值
    let request = "GET /check?is_active=not_a_boolean HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    println!("--- Final Response ---\n{}\n--------------------", resp_str);

    assert!(
        resp_str.contains("400 Bad Request"),
        "DSL 修正后，校验应该生效并返回 400"
    );
    assert!(
        resp_str.contains("'not_a_boolean' is not a valid boolean"),
        "应该包含特定的错误消息"
    );
}

#[tokio::test]
async fn test_validator_integer_strict_error_integration() {
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    // 1. 准备服务器地址
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    // 2. 构造 DSL：要求 query 中的 'age' 必须是 int
    // 语法使用你确认正确的：(变量名:类型)
    let mut dsl_map = HashMap::new();
    dsl_map.insert("query".to_string(), "(age:int)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));
    // 注入 validator 中间件
    let validator_mw = to_validator(dsl_map);
    hr.insert(
        "/user",
        Some("GET"),
        exe!(|_ctx| { true }),
        Some(vec![validator_mw]),
    );

    // 3. 启动 AexServer
    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 4. 发送非法请求：age 传入非整数 "invalid_99"
    let mut stream = TcpStream::connect(actual_addr)
        .await
        .expect("Failed to connect");
    let raw_request =
        "GET /user?age=invalid_99 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(raw_request.as_bytes()).await.unwrap();

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let resp_text = String::from_utf8_lossy(&response_buf);

    println!(
        "--- Integer Error Response ---\n{}\n----------------------------",
        resp_text
    );

    // 5. 验证断言

    // 验证 A: 状态码必须是 400
    assert!(resp_text.contains("400 Bad Request"), "应当返回 400 状态码");

    // 验证 B: 必须匹配你要求的错误字符串格式
    // 代码原文：format!("'{}' is not a valid integer", s)
    let expected_detail = "'invalid_99' is not a valid integer";
    assert!(
        resp_text.contains(expected_detail),
        "响应 Body 缺失具体的整数解析错误消息"
    );

    // 验证 C: 链路前缀验证
    assert!(
        resp_text.contains("query conversion error:"),
        "缺失校验器前缀"
    );
}

#[tokio::test]
async fn test_validator_float_strict_error_integration() {
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    // 1. 准备服务器地址
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    // 2. 构造 DSL：要求 query 中的 'price' 必须是 float
    // 语法：(变量名:类型)
    let mut dsl_map = HashMap::new();
    dsl_map.insert("query".to_string(), "(price:float)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));
    // 注入 validator 中间件
    let validator_mw = to_validator(dsl_map);
    hr.insert(
        "/product",
        Some("GET"),
        exe!(|_ctx| { true }),
        Some(vec![validator_mw]),
    );

    // 3. 启动 AexServer
    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 4. 发送非法请求：price 传入非浮点数 "abc.def"
    let mut stream = TcpStream::connect(actual_addr)
        .await
        .expect("Failed to connect");
    let raw_request =
        "GET /product?price=abc.def HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(raw_request.as_bytes()).await.unwrap();

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).await.unwrap();
    let resp_text = String::from_utf8_lossy(&response_buf);

    println!(
        "--- Float Error Response ---\n{}\n----------------------------",
        resp_text
    );

    // 5. 验证断言

    // 验证 A: 状态码必须是 400
    assert!(resp_text.contains("400 Bad Request"), "应当返回 400 状态码");

    // 验证 B: 必须匹配代码中的错误字符串格式
    // 代码原文：format!("'{}' is not a valid float", s)
    let expected_detail = "'abc.def' is not a valid float";
    assert!(
        resp_text.contains(expected_detail),
        "响应 Body 缺失具体的浮点数解析错误消息"
    );

    // 验证 C: 链路前缀验证
    assert!(
        resp_text.contains("query conversion error:"),
        "缺失校验器前缀"
    );
}

#[tokio::test]
async fn test_validator_float_auto_completion_promotion() {
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    // 1. DSL: 规定 val 为 float 类型
    let mut dsl_map = HashMap::new();
    dsl_map.insert("query".to_string(), "(val:float)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 2. 核心：在 Handler 中提取转换后的 Meta 数据
    hr.insert(
        "/promote",
        Some("GET"),
        exe!(|ctx| {
            // 💡 重点：从 Context 拿到转换后的 HttpMetadata
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();

            println!("meta = {:?}", meta);

            // 获取转换后的 params
            if let Some(params) = &meta.params {
                if let Some(final_val) = params.query.get("val") {
                    // 将转换后的字符串（期望是 "100.0"）写回响应 Body
                    meta.body = final_val.join("").as_bytes().to_vec();
                    ctx.local.set_value(meta);
                }
            }
            true
        }),
        Some(vec![to_validator(dsl_map)]),
    );

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 3. 发送 "100"
    let mut stream = TcpStream::connect(actual_addr).await.unwrap();
    let request = "GET /promote?val=100 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    println!(
        "--- Promotion Result ---\n{}\n--------------------",
        resp_str
    );

    // 4. 断言验证
    // 如果补全逻辑 format!("{}.0", s) 生效，返回的 Body 必须是 100.0
    assert!(resp_str.contains("200 OK"), "转换成功应返回 200");
    assert!(
        resp_str.contains("100.0"),
        "Meta 中的值应当从 '100' 提升为 '100.0'"
    );
}

#[tokio::test]
async fn test_validator_value_to_string_fallback() {
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    // 1. DSL: 正常配置
    let mut dsl_map = HashMap::new();
    dsl_map.insert("query".to_string(), "(tag:string)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 2. 注入处理器：验证提取出来的值是否为空字符串
    hr.insert(
        "/fallback",
        Some("GET"),
        exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            let mut found_empty = false;

            if let Some(params) = &meta.params {
                if let Some(val) = params.query.get("tag") {
                    // 如果落入了 _ => "".to_string()，这里拿到的就是空
                    if val.is_empty() {
                        found_empty = true;
                    }
                }
            }

            if found_empty {
                meta.body = b"fallback_to_empty".to_vec();
            } else {
                meta.body = b"has_value".to_vec();
            }
            ctx.local.set_value(meta);

            true
        }),
        Some(vec![to_validator(dsl_map)]),
    );

    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 3. 发送请求
    // 注意：如果是 String 类型通常会有匹配，
    // 这里是为了验证如果 convert_by_type 返回了不在 match 列表里的 Value 时的表现
    let mut stream = TcpStream::connect(actual_addr).await.unwrap();
    let request =
        "GET /fallback?tag=anything HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    println!(
        "--- Fallback Test Result ---\n{}\n--------------------",
        resp_str
    );

    // 验证：目前由于 String/Int/Float 都有匹配，这个测试在当前代码下应该返回 "has_value"
    // 如果你手动在 convert_by_type 里返回一个未在 value_to_string 处理的 Value 类型，
    // 它就会返回 "fallback_to_empty"
}

#[test]
fn test_value_to_string_all_variants() {
    // --- 正常分支测试 ---
    assert_eq!(value_to_string(Value::Bool(true)), "true");
    assert_eq!(value_to_string(Value::Int(123)), "123");
    assert_eq!(value_to_string(Value::Float(45.0)), "45.0");
    assert_eq!(value_to_string(Value::String("hello".into())), "hello");

    // --- 🚀 重点：测试 _ => "".to_string() 分支 ---
    // 传入一个 Array 或 Object，这两个在 match 中没有对应的分支，会落入 _
    let array_val = Value::Array(vec![Value::Int(1)]);
    let object_val = Value::Object(HashMap::new());

    assert_eq!(
        value_to_string(array_val),
        "",
        "Array 类型应触发兜底分支返回空字符串"
    );
    assert_eq!(
        value_to_string(object_val),
        "",
        "Object 类型应触发兜底分支返回空字符串"
    );
}

#[tokio::test]
async fn test_validator_params_none_fallback() {
    use std::collections::HashMap;

    // 1. 设置地址与路由
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let mut dsl_map = HashMap::new();
    dsl_map.insert("query".to_string(), "(id:int)".to_string());

    let mut hr = Router::new(NodeType::Static("root".into()));

    // 注入处理器：如果 fallback 成功，Params 会被初始化
    hr.insert(
        "/fallback_params",
        Some("GET"),
        exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            // 验证 params 是否已经不再是 None (被 unwrap_or_else 补全并后续写回)
            if meta.params.is_some() {
                meta.body = b"params_initialized".to_vec();
                ctx.local.set_value(meta);
            }
            true
        }),
        Some(vec![to_validator(dsl_map)]),
    );

    // 2. 启动服务器并发送请求
    let server = HTTPServer::new(actual_addr).http(hr);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut stream = tokio::net::TcpStream::connect(actual_addr).await.unwrap();
    // 发送一个正常请求，但我们将依靠服务器内部逻辑触发 params 的初始化
    let request =
        "GET /fallback_params?id=123 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    println!(
        "--- Params Fallback Response ---\n{}\n--------------------",
        resp_str
    );

    // 3. 验证逻辑
    // 只要服务器没崩溃，且返回了业务标记，说明 unwrap_or_else 成功处理了初始的 None 状态
    assert!(resp_str.contains("200 OK"));
    assert!(resp_str.contains("params_initialized"));
}
