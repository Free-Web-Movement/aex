use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aex::http::router::Router;
use aex::http::static_files::StaticFiles;
use aex::server::Server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::sleep;

fn unique_tmp_dir() -> std::path::PathBuf {
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("aex_static_test_{}_{}", std::process::id(), uniq))
}

struct TestServer {
    addr: SocketAddr,
    _dir: std::path::PathBuf,
}

async fn start(dir: std::path::PathBuf) -> TestServer {
    let mut router = Router::default();
    router.static_files("/static", &dir);
    router.static_files_with("/small", StaticFiles::new(&dir).max_file_size(10));
    start_router(router).await
}

async fn start_router(router: Router) -> TestServer {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let server = Server::new(actual_addr, None);
    let server = server.http(router).clone();
    tokio::spawn(async move {
        let _ = server.start().await;
    });

    TestServer {
        addr: actual_addr,
        _dir: std::path::PathBuf::new(),
    }
}

async fn get(addr: SocketAddr, path: &str) -> Option<(u16, Vec<u8>, String)> {
    for _ in 0..20 {
        sleep(Duration::from_millis(50)).await;
        if let Ok(r) = reqwest::Client::new()
            .get(format!("http://{}{}", addr, path))
            .send()
            .await
        {
            let status = r.status().as_u16();
            let ct = r
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let bytes = r.bytes().await.unwrap().to_vec();
            return Some((status, bytes, ct));
        }
    }
    None
}

#[tokio::test]
async fn test_static_files_basic_site() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join("assets").join("img").join("icons")).unwrap();
    std::fs::write(dir.join("index.html"), "<h1>home</h1>").unwrap();
    std::fs::write(dir.join("app.css"), "body{}").unwrap();
    std::fs::write(dir.join("app.js"), "console.log(1)").unwrap();
    std::fs::write(dir.join("notes.md"), "# 标题").unwrap();
    std::fs::write(dir.join("img.png"), [0x89u8, b'P', b'N', b'G', 0, 1, 2]).unwrap();
    std::fs::write(dir.join("sub").join("index.html"), "<h1>sub</h1>").unwrap();
    std::fs::write(
        dir.join("assets").join("img").join("icons").join("fav.ico"),
        [0u8, 1],
    )
    .unwrap();

    let s = start(dir).await;

    // 前缀本身 -> index.html
    let (status, body, ct) = get(s.addr, "/static").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "<h1>home</h1>");
    assert_eq!(ct, "text/html; charset=utf-8");

    // 尾部斜杠 -> index.html
    let (status, body, _) = get(s.addr, "/static/").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "<h1>home</h1>");

    // css / js 自动识别 MIME
    let (status, _, ct) = get(s.addr, "/static/app.css").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(ct, "text/css; charset=utf-8");
    let (status, _, ct) = get(s.addr, "/static/app.js").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(ct, "application/javascript");

    // 二进制内容原样返回
    let (status, body, ct) = get(s.addr, "/static/img.png").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(body, [0x89u8, b'P', b'N', b'G', 0, 1, 2]);
    assert_eq!(ct, "image/png");

    // 文本文件（md/txt）默认带 utf-8 编码
    let (status, body, ct) = get(s.addr, "/static/notes.md").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "# 标题");
    assert_eq!(ct, "text/plain; charset=utf-8");

    // 子目录入口文件
    let (status, body, _) = get(s.addr, "/static/sub").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "<h1>sub</h1>");

    // 递归：任意深度的下级目录都可访问
    let (status, body, ct) = get(s.addr, "/static/assets/img/icons/fav.ico")
        .await
        .expect("no response");
    assert_eq!(status, 200);
    assert_eq!(body, [0u8, 1]);
    assert_eq!(ct, "image/x-icon");

    // 不存在的文件 -> 404
    let (status, _, _) = get(s.addr, "/static/missing.txt")
        .await
        .expect("no response");
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_static_files_size_limit() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    // 20 字节文件，但 /small 上限只有 10 字节
    std::fs::write(dir.join("big.txt"), "x".repeat(20)).unwrap();

    let s = start(dir).await;

    let (status, _, _) = get(s.addr, "/small/big.txt").await.expect("no response");
    assert_eq!(status, 404);

    // 默认 100MiB 上限下，小文件正常
    let (status, _, _) = get(s.addr, "/static/big.txt").await.expect("no response");
    assert_eq!(status, 200);
}

#[tokio::test]
async fn test_static_files_dir_listing() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(dir.join("assets").join("img")).unwrap();
    std::fs::write(dir.join("app.js"), "console.log(1)").unwrap();
    std::fs::write(dir.join("readme.txt"), "hello").unwrap();
    std::fs::write(dir.join("song.mp3"), "x").unwrap();
    std::fs::write(dir.join("archive.zip"), "x").unwrap();
    // 有 index.html 的目录不应生成列表页
    std::fs::create_dir_all(dir.join("home")).unwrap();
    std::fs::write(dir.join("home").join("index.html"), "<h1>home</h1>").unwrap();

    let mut router = Router::default();
    router.static_files("/static", &dir);
    let s = start_router(router).await;

    // 无 index.html 的目录 -> 生成列表页
    let (status, body, ct) = get(s.addr, "/static").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(ct, "text/html; charset=utf-8");
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Index of /static"), "got: {html}");
    assert!(html.contains("app.js"), "got: {html}");
    assert!(html.contains("readme.txt"), "got: {html}");
    assert!(html.contains("assets/"), "got: {html}");
    assert!(html.contains("home/"), "got: {html}");
    // 文件类型图标：目录用文件夹图标，不同扩展名各有专属图标
    assert!(html.contains("📁"), "dir icon missing: {html}");
    assert!(html.contains("🟨"), "js icon missing: {html}");
    assert!(html.contains("📄"), "txt icon missing: {html}");
    assert!(html.contains("🎵"), "mp3 icon missing: {html}");
    assert!(html.contains("📦"), "zip icon missing: {html}");
    // 根目录（挂载前缀本身）不显示上级链接
    assert!(
        !html.contains("../"),
        "root should have no parent link: {html}"
    );

    // 子目录列表：显示 ../ 上级链接，可递归进入
    let (status, body, _) = get(s.addr, "/static/assets").await.expect("no response");
    assert_eq!(status, 200);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Index of /static/assets"), "got: {html}");
    assert!(html.contains("href=\"../\""), "got: {html}");
    assert!(html.contains("img/"), "got: {html}");

    // 主流语言文件各有专属图标
    std::fs::write(dir.join("assets").join("main.rs"), "fn main(){}").unwrap();
    std::fs::write(dir.join("assets").join("main.go"), "package main").unwrap();
    std::fs::write(dir.join("assets").join("main.c"), "int main(){}").unwrap();
    std::fs::write(dir.join("assets").join("main.rb"), "puts 1").unwrap();
    std::fs::write(dir.join("assets").join("page.html"), "<h1>hi</h1>").unwrap();
    let (status, body, _) = get(s.addr, "/static/assets").await.expect("no response");
    assert_eq!(status, 200);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("🦀"), "rust icon missing: {html}");
    assert!(html.contains("🐹"), "go icon missing: {html}");
    assert!(html.contains("🅲"), "c icon missing: {html}");
    assert!(html.contains("💎"), "ruby icon missing: {html}");
    assert!(html.contains("🌐"), "html icon missing: {html}");

    // 递归：再深入一层（/static/assets/img）仍是列表页
    let (status, body, _) = get(s.addr, "/static/assets/img")
        .await
        .expect("no response");
    assert_eq!(status, 200);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Index of /static/assets/img"), "got: {html}");
    assert!(html.contains("href=\"../\""), "got: {html}");

    // 有 index.html 的目录仍回退到入口文件，不生成列表
    let (status, body, _) = get(s.addr, "/static/home").await.expect("no response");
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(body).unwrap(), "<h1>home</h1>");
}

async fn wait_ready(addr: SocketAddr) {
    for _ in 0..40 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("server not reachable at {addr}");
}

#[tokio::test]
async fn test_static_files_dir_redirect() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("a.txt"), "a").unwrap();

    let mut router = Router::default();
    router.static_files("/static", &dir);
    let s = start_router(router).await;
    wait_ready(s.addr).await;

    // 目录无尾部斜杠 -> 301 到尾部斜杠，保证相对链接解析正确
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let r = client
        .get(format!("http://{}/static/sub", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 301);
    assert_eq!(
        r.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap(),
        "/static/sub/"
    );

    // 带 query 也不丢失
    let r = client
        .get(format!("http://{}/static/sub?x=1&y=2", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 301);
    assert_eq!(
        r.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap(),
        "/static/sub/?x=1&y=2"
    );

    // 尾部斜杠 -> 直接列表，无重定向
    let (status, body, _) = get(s.addr, "/static/sub/").await.expect("no response");
    assert_eq!(status, 200);
    assert!(String::from_utf8(body).unwrap().contains("a.txt"));

    // 根目录无尾部斜杠也重定向
    let r = client
        .get(format!("http://{}/static", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 301);
    assert_eq!(
        r.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap(),
        "/static/"
    );
}

#[tokio::test]
async fn test_static_files_symlink_escape_blocked() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "ok").unwrap();
    // 越界文件 + 目录内的 symlink 指向它
    let parent = dir.parent().unwrap();
    let secret = parent.join("secret_link_target.txt");
    std::fs::write(&secret, "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, dir.join("link.txt")).unwrap();

    let mut router = Router::default();
    router.static_files("/static", &dir);
    let s = start_router(router).await;

    let (status, _, _) = get(s.addr, "/static/link.txt").await.expect("no response");
    assert_eq!(status, 404, "symlink escaping root must be blocked");

    let (status, _, _) = get(s.addr, "/static/index.html")
        .await
        .expect("no response");
    assert_eq!(status, 200, "normal file must still be served");
}

#[tokio::test]
async fn test_static_files_traversal_blocked() {
    let dir = unique_tmp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "ok").unwrap();
    // 越界文件：放在根目录之外
    let parent = dir.parent().unwrap();
    std::fs::write(parent.join("secret.txt"), "secret").unwrap();

    let s = start(dir).await;

    // 原始 TCP 请求，避免客户端路径归一化，直接验证服务端防护
    let mut stream = None;
    for _ in 0..20 {
        if let Ok(st) = tokio::net::TcpStream::connect(s.addr).await {
            stream = Some(st);
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    let mut stream = stream.expect("server not reachable");
    stream
        .write_all(b"GET /static/../secret.txt HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read error");
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.starts_with("HTTP/1.1 404"), "got: {}", head);
}
