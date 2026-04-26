use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

use aex::connection::global::GlobalContext;
use aex::connection::manager::ConnectionManager;
use aex::connection::node::Node;

fn create_global(addr: SocketAddr) -> Arc<GlobalContext> {
    Arc::new(GlobalContext::new(addr, None))
}

#[tokio::test]
async fn test_p2p_node_entry_basic() {
    let node = Node::from_system(8080, vec![0x11u8; 32], 1);

    assert_eq!(node.id, vec![0x11u8; 32]);
    assert_eq!(node.port, 8080);
    assert_eq!(node.version, 1);

    let ips = node.get_all();
    assert!(!ips.is_empty());
}
