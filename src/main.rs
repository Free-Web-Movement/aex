//! `http-server` — 一个开箱即用的静态文件服务器。
//!
//! 在任意目录运行，即可把该目录作为 HTTP 静态资源站发布：
//!
//! ```bash
//! http-server                 # 当前目录，端口 8080
//! http-server 3000            # 当前目录，端口 3000
//! http-server ./public        # 发布 ./public
//! http-server ./public 3000   # 发布 ./public，端口 3000
//! ```
//!
//! 特性：自动识别 MIME（html/css/js/图片等，文本默认带 utf-8 编码）、
//! 递归访问所有子目录、目录列表页带文件类型图标、目录回退 index.html、
//! 禁止访问根目录之外的路径、单文件不超过 100 MiB。
//! 端口被占用时自动向后递增寻找可用端口。

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;

use aex::http::router::Router as HttpRouter;
use aex::server::HTTPServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let (dir, mut port): (PathBuf, u16) = match args.len() {
        0 => (std::env::current_dir()?, 8080),
        1 => match args[0].parse::<u16>() {
            // 单个数字参数视为端口
            Ok(p) => (std::env::current_dir()?, p),
            Err(_) => (args[0].clone().into(), 8080),
        },
        _ => (args[0].clone().into(), args[1].parse()?),
    };

    let dir = std::fs::canonicalize(if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(dir)
    })?;

    // 端口占用检测：被占用则 +1 重试，直到找到可用端口。
    let addr = loop {
        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
        if std::net::TcpListener::bind(addr).is_ok() {
            break SocketAddr::V4(addr);
        }
        println!("Port {port} in use, trying {}", port + 1);
        port = port
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("No free port"))?;
    };

    let mut router = HttpRouter::default();
    router.static_files("/", &dir);

    println!("Serving {} at http://localhost:{port}", dir.display());
    if let Ok(ifaces) = get_if_addrs::get_if_addrs() {
        for i in ifaces {
            if !i.is_loopback() && i.ip().is_ipv4() {
                println!("  http://{}:{port}", i.ip());
            }
        }
    }
    println!("Press Ctrl+C to stop.");

    HTTPServer::new(addr, None).http(router).start().await?;
    tokio::signal::ctrl_c().await?;
    Ok(())
}
