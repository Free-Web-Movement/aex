#[cfg(test)]
mod tests {
    #[test]
    fn test_udp_router_creation() {
        let router = aex::udp::router::Router::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_udp_exports() {
        let _ = aex::udp::router::Router::new();
    }

    #[tokio::test]
    async fn test_udp_socket_binding() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_addr = socket.local_addr().unwrap();
        assert!(local_addr.port() > 0);
    }
}