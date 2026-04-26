use aex::connection::global::GlobalContext;
use aex::exe;
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::tcp::router::Router as TcpRouter;
use aex::tcp::types::{Command, RawCodec};
use aex::udp::router::Router as UdpRouter;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let mut http_router = HttpRouter::default();

    http_router
        .get(
            "/",
            exe!(|ctx| {
                ctx.send("Hello world!", None);
                true
            }),
        )
        .register();

    let tcp_router = {
        let router = TcpRouter::<RawCodec, RawCodec>::new();
        let mut router = router.extractor(|c: &RawCodec| c.id());
        router.on::<RawCodec, RawCodec>(
            1001,
            Box::new(|_ctx, _frame: RawCodec, cmd: RawCodec| {
                Box::pin(async move {
                    let _cmd = cmd.clone();
                    println!("Handling command...");
                    Ok(true)
                })
            }),
            vec![],
        );
        router
    };

    let udp_router = {
        let router = UdpRouter::<RawCodec, RawCodec>::new();
        let mut router = router.extractor(|c: &RawCodec| c.id());
        router.on(
            2002,
            |_global: Arc<GlobalContext>,
             _frame: RawCodec,
             payload: RawCodec,
             peer,
             socket: Arc<UdpSocket>| async move {
                println!("[UDP] Received 2002 from {}, data: {:?}", peer, payload);
                let response = b"UDP ACK".to_vec();
                let _ = socket.send_to(&response, peer).await;
                Ok(true)
            },
        );
        router
    };

    Server::new(addr, None)
        .http(http_router)
        .tcp(tcp_router)
        .udp(udp_router)
        .start_with_protocols::<RawCodec, RawCodec>()
        .await?;

    Ok(())
}
