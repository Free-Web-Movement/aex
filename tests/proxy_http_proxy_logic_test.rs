#[cfg(feature = "proxy")]
mod proxy_logic {
    use aex::connection::context::Context;
    use aex::connection::global::GlobalContext;
    use aex::http::meta::HttpMetadata;
    use aex::http::protocol::method::HttpMethod;
    use aex::proxy::http_proxy::maybe_handle_http_proxy;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // 通过 maybe_handle_http_proxy 的公开行为间接测试内部纯逻辑：
    // - 无 metadata → false
    // - https:// 绝对形式 → 502
    // - CONNECT 无 authorizer → 走 connect（open proxy，尝试连接上游会 504/502）

    #[tokio::test]
    async fn no_metadata_returns_false() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));
        let mut ctx = Context::new(None, None, global, addr);
        let handled = maybe_handle_http_proxy(&mut ctx, None).await;
        assert!(!handled);
    }

    #[tokio::test]
    async fn https_absolute_form_returns_502() {
        use tokio::io::AsyncReadExt;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));
        let (mut client_read, writer) = tokio::io::duplex(1024);
        let mut meta = HttpMetadata::new();
        meta.method = HttpMethod::GET;
        meta.path = "https://example.com/path".to_string();
        let mut ctx = Context::new(None, Some(Box::new(writer)), global, addr);
        ctx.local.set_value(meta);

        let handled = maybe_handle_http_proxy(&mut ctx, None).await;
        assert!(handled, "https absolute-form should be handled");

        let mut buf = [0u8; 256];
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client_read.read(&mut buf),
        )
        .await
        .expect("read timeout")
        .expect("read error");
        let resp = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
    }

    #[tokio::test]
    async fn origin_form_returns_false() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));
        let mut meta = HttpMetadata::new();
        meta.method = HttpMethod::GET;
        meta.path = "/plain/path".to_string();
        let mut ctx = Context::new(None, None, global, addr);
        ctx.local.set_value(meta);

        let handled = maybe_handle_http_proxy(&mut ctx, None).await;
        assert!(!handled, "origin-form is website traffic");
    }

    #[tokio::test]
    async fn connect_to_unreachable_returns_502() {
        use tokio::io::AsyncReadExt;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));
        let (mut client_read, writer) = tokio::io::duplex(1024);
        let mut meta = HttpMetadata::new();
        meta.method = HttpMethod::CONNECT;
        meta.path = "127.0.0.1:1".to_string(); // 端口 1 几乎必然不可达
        let mut ctx = Context::new(None, Some(Box::new(writer)), global, addr);
        ctx.local.set_value(meta);

        let handled = maybe_handle_http_proxy(&mut ctx, None).await;
        assert!(handled, "CONNECT should be handled");

        let mut buf = [0u8; 256];
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_read.read(&mut buf),
        )
        .await
        .expect("read timeout")
        .expect("read error");
        let resp = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(resp.starts_with("HTTP/1.1 502"), "got {resp}");
    }
}
