//! HTTP Server Benchmark - Actix-web
//!
//! Tests: no URL, static URL, dynamic URL

use actix_web::{App, HttpServer, Responder, get, web};

#[get("/")]
async fn root() -> impl Responder {
    "Hello"
}

#[get("/api/users")]
async fn users() -> impl Responder {
    r#"[{"id":1,"name":"alice"}]"#
}

#[get("/api/users/{id}")]
async fn user_id(id: web::Path<i32>) -> impl Responder {
    format!(r#"{{"id":{}}}"#, id)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new().service(root).service(users).service(user_id))
        .bind("127.0.0.1:8082")?
        .run()
        .await
}
