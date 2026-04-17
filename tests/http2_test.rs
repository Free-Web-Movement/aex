use aex::http2::H2Codec;
use aex::server::Server;
use std::net::SocketAddr;
use std::sync::Arc;

use aex::connection::global::GlobalContext;
use aex::http::router::Router as HttpRouter;

#[test]
fn test_h2_codec_new() {
    let router = HttpRouter::new(aex::http::router::NodeType::Static("root".into()));
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let _codec = H2Codec::new(Arc::new(router), global);
}
