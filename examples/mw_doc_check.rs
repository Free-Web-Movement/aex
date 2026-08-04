//! 验证 README 中间件文档示例可编译
use std::sync::Arc;

use aex::connection::context::Context;
use aex::http::meta::HttpMetadata;
use aex::http::middlewares::rate_limit::RateLimitConfig;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::status::StatusCode;
use aex::http::router::Router;
use aex::http::types::{Executor, IntoExecutor};

fn main() {
    let mut router = Router::default();

    // 同步闭包中间件
    let auth = |ctx: &mut Context| {
        if ctx.req().query("token").as_deref() == Some("secret") {
            true
        } else {
            ctx.status(StatusCode::Unauthorized).text("forbidden");
            false
        }
    };

    // 异步中间件
    let rate_limiter = aex::exe!(|ctx| {
        let _ = ctx;
        true
    });

    // 可复用：返回 Arc<Executor>
    pub struct RateLimit {}
    impl RateLimit {
        pub fn build() -> Arc<Executor> {
            IntoExecutor::into_executor(move |ctx: &mut Context| {
                let _ = ctx;
                true
            })
        }
    }

    let handler = |ctx: &mut Context| {
        ctx.text("ok");
    };

    // 三种挂载
    router.get("/x", handler).middleware(rate_limiter);
    router.get_with(
        "/y",
        [IntoExecutor::into_executor(auth), RateLimit::build()],
        |_| "ok",
    );
    router.get("/z", |_| "z");

    // 对象中间件：裸标识符 -> &self 方法，能读实例状态
    struct Api {
        api_key: String,
    }

    #[aex::routes]
    impl Api {
        // 对象中间件与内置中间件（模块级表达式）可混排
        #[get("/admin", [auth, RateLimitConfig::new(100, 60).build()])]
        fn admin(&self, ctx: &mut Context) {
            ctx.text("admin-only");
        }

        fn auth(&self, ctx: &mut Context) -> bool {
            let key = HeaderKey::from_str("x-api-key").unwrap();
            ctx.get::<HttpMetadata>()
                .map(|m| m.headers.get(&key).is_some_and(|v| v == &self.api_key))
                .unwrap_or(false)
        }
    }

    router.push(Api {
        api_key: "secret".into(),
    });
}
