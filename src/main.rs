use std::net::SocketAddr;
use std::sync::Arc;
use aex::connection::context::{HttpMetadata, TypeMapExt};
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::media_type::{MediaType, SubMediaType};
use aex::http::protocol::status::StatusCode;
use aex::{ get, route };
use aex::tcp::types::{ RawCodec, Command };
use aex::server::AexServer;
use aex::http::router::{ NodeType, Router as HttpRouter };
use aex::tcp::router::Router as TcpRouter;
use aex::udp::router::Router as UdpRouter;
use futures::FutureExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    // --- 1. HTTP 路由配置 ---
    let mut http_router = HttpRouter::new(NodeType::Static("root".into()));

    route!(
        http_router,
        get!("/", |ctx: &mut HTTPContext| {
            Box::pin(async move {
                let meta = &mut ctx.meta_out;
                meta.status = StatusCode::Ok;
                meta.headers.insert(HeaderKey::ContentType, SubMediaType::Plain.as_str().to_string());
                meta.body = "Hello world!".to_string().into_bytes().to_vec();
                // false = 不继续 middleware（如果你还保留这个语义）
                true
            }).boxed()
        })
    );

    // --- 2. TCP 路由配置 (使用 RawCodec) ---
    // 提取器：取二进制前4字节作为 ID
    let mut tcp_router = TcpRouter::<RawCodec, RawCodec, u32>::new(|c| c.id());

    // 注册 TCP 指令 1001
    tcp_router.on(1001, |cmd, reader, writer| async move {
        println!("[TCP] Received 1001, payload len: {}", cmd.0.len());
        // 这里可以继续使用 reader/writer 进行长连接交互
        Ok(true)
    });

    // --- 3. UDP 路由配置 (使用 RawCodec) ---
    let mut udp_router = UdpRouter::<RawCodec, RawCodec, u32>::new(|c| c.id());

    // 注册 UDP 指令 2002
    udp_router.on(2002, |payload, peer, socket| async move {
        println!("[UDP] Received 2002 from {}, data: {:?}", peer, payload);
        // UDP 回包示例
        let response = b"UDP ACK".to_vec();
        let _ = socket.send_to(&response, peer).await;
        Ok(true)
    });

    // --- 4. 组装并启动服务器 ---
    // 借力于 HTTPServer 类型别名或直接使用 AexServer
    AexServer::new(addr).http(http_router).tcp(tcp_router).udp(udp_router).start().await?;

    Ok(())
}
