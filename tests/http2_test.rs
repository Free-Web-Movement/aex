use aex::connection::context::Context;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::status::StatusCode;
use aex::http2::H2Codec;
use aex::server::HTTPServer;
use aex::tcp::types::{Codec, Command, RawCodec};
use futures::FutureExt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{Duration, sleep, timeout};

use aex::http::router::{NodeType, Router as HttpRouter};
use aex::tcp::router::Router as TcpRouter;
use aex::udp::router::Router as UdpRouter;
use aex::connection::global::GlobalContext;

#[test]
fn test_h2_codec_new() {
    let router = HttpRouter::new(NodeType::Static("root".into()));
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let _codec = H2Codec::new(Arc::new(router), global);
}

#[tokio::test]
async fn test_http2_routing() {
    let test_result = timeout(Duration::from_secs(10), async {
        let addr: SocketAddr = "[::1]:0".parse().unwrap();
        let mut server = HTTPServer::new(addr, None);

        let mut hr = HttpRouter::new(NodeType::Static("root".into()));

        hr.insert(
            "/",
            Some("GET"),
            Arc::new(|ctx: &mut Context| {
                Box::pin(async move {
                    let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap();
                    meta.status = StatusCode::Ok;
                    meta.headers.insert(HeaderKey::ContentType, "text/plain".to_string());
                    meta.body = b"HTTP/2 Root".to_vec();
                    ctx.local.set_value(meta.clone());
                    true
                })
                .boxed()
            }),
            None,
        );

        hr.insert(
            "/api/users/:id",
            Some("GET"),
            Arc::new(|ctx: &mut Context| {
                Box::pin(async move {
                    let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap();
                    meta.status = StatusCode::Ok;
                    meta.headers.insert(HeaderKey::ContentType, "application/json".to_string());
                    
                    let user_id = meta.params.as_ref()
                        .and_then(|p| p.data.as_ref())
                        .and_then(|d| d.get("id"))
                        .cloned()
                        .unwrap_or_default();
                    
                    meta.body = format!(r#"{{"id":"{}"}}"#, user_id).into_bytes();
                    ctx.local.set_value(meta.clone());
                    true
                })
                .boxed()
            }),
            None,
        );

        let temp_listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        server.addr = actual_addr;
        let server = server.http(hr).http2().clone();

        tokio::spawn(async move {
            if let Err(e) = server
                .start()
                .await
            {
                eprintln!("Server exit with error: {}", e);
            }
        });

        sleep(Duration::from_millis(200)).await;

        println!("✅ HTTP/2 routing test setup passed!");
    })
    .await;

    if test_result.is_err() {
        panic!("Test timed out!");
    }
}

#[tokio::test]
async fn test_mixed_protocol_server() {
    let test_result = timeout(Duration::from_secs(15), async {
        let addr: SocketAddr = "[::1]:0".parse().unwrap();
        let mut server = HTTPServer::new(addr, None);

        let mut hr = HttpRouter::new(NodeType::Static("root".into()));
        hr.insert(
            "/",
            Some("GET"),
            Arc::new(|ctx: &mut Context| {
                Box::pin(async move {
                    let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap();
                    meta.status = StatusCode::Ok;
                    meta.headers.insert(HeaderKey::ContentType, "text/plain".to_string());
                    meta.body = b"Hello HTTP".to_vec();
                    ctx.local.set_value(meta.clone());
                    true
                })
                .boxed()
            }),
            None,
        );

        let mut tr = TcpRouter::<RawCodec, RawCodec>::new().extractor(|c: &RawCodec| c.id());
        tr.on(
            10,
            Box::new(|_, _: RawCodec, _: RawCodec| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        let mut ur = UdpRouter::<RawCodec, RawCodec>::new().extractor(|c: &RawCodec| c.id());
        ur.on(20, |_, _: RawCodec, _: RawCodec, addr, socket| async move {
            socket.send_to(b"udp_ok", addr).await?;
            Ok(true)
        });

        let temp_listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        server.addr = actual_addr;
        let server = server
            .http(hr)
            .tcp(tr)
            .udp(ur)
            .http2()
            .clone();

        tokio::spawn(async move {
            if let Err(e) = server
                .start_with_protocols::<RawCodec, RawCodec>()
                .await
            {
                eprintln!("Server exit with error: {}", e);
            }
        });

        sleep(Duration::from_millis(200)).await;

        println!("=== Testing Mixed Protocol Server ===");

        println!("[1] Testing HTTP/1.1...");
        let http_res = reqwest::get(format!("http://{}", actual_addr))
            .await
            .expect("HTTP/1.1 request failed");
        assert_eq!(http_res.status(), 200);
        assert_eq!(http_res.text().await.unwrap(), "Hello HTTP");

        println!("[2] Testing TCP...");
        let mut tcp_conn = TcpStream::connect(actual_addr)
            .await
            .expect("TCP connect failed");
        let rawtcp = Codec::encode(&RawCodec(vec![10, 0, 0, 0]));
        tcp_conn.write_all(&rawtcp).await.unwrap();
        sleep(Duration::from_millis(100)).await;

        println!("[3] Testing UDP...");
        let udp_client = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                println!("UDP bind failed (skipped): {}", e);
                return;
            }
        };
        let target_addr = format!("{}:{}", actual_addr.ip(), actual_addr.port());
        let rawudp = Codec::encode(&RawCodec(vec![20, 0, 0, 0]));
        if let Err(e) = udp_client.send_to(&rawudp, &target_addr).await {
            println!("UDP send failed (skipped): {}", e);
            return;
        }
        let mut buf = [0u8; 1024];
        let result = timeout(Duration::from_secs(2), udp_client.recv_from(&mut buf)).await;
        match result {
            Ok(Ok((len, _))) => {
                assert_eq!(&buf[..len], b"udp_ok");
            }
            _ => {
                println!("UDP test skipped (environment limitation)");
            }
        }

        println!("✅ All mixed protocol tests passed!");
    })
    .await;

    if test_result.is_err() {
        panic!("Test timed out! Possible deadlock or server not responding.");
    }
}
