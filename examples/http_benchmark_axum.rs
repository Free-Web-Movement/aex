//! HTTP Server Benchmark - Axum
//!
//! Tests: no URL, static URL, dynamic URL

use axum::{routing::get, Router};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8081));

    let app = Router::new()
        .route("/", get(handler_root))
        .route("/api/users", get(handler_users))
        .route("/api/users/:id", get(handler_user_id));

    println!("Axum server on {}", addr);
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app,
    ).await.unwrap();
}

async fn handler_root() -> &'static str {
    "Hello"
}

async fn handler_users() -> &'static str {
    r#"[{"id":1,"name":"alice"}]"#
}

async fn handler_user_id(axum::extract::Path(id): axum::extract::Path<i32>) -> String {
    format!(r#"{{"id":{}}}"#, id)
}