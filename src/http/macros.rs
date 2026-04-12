//! # HTTP Macros
//!
//! Macros for defining HTTP handlers.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aex::exe;
//!
//! let handler = exe!(|ctx| {
//!     ctx.send("response");
//!     true
//! });
//! ```

// for `.boxed()`

#[macro_export]
macro_rules! exe {
    // 支持 move 闭包
    (move | $ctx:ident | $body:block) => {{
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::Context;
        use $crate::http::types::Executor;

        Arc::new(move |$ctx: &mut Context| async move { $body }.boxed())
    }};

    // 支持 move 闭包 + pre 处理
    (move | $ctx:ident, $data:ident | $body:block, | $pre_ctx:ident | $pre:block) => {{
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::Context;
        use $crate::http::types::Executor;

        Arc::new(move |$ctx: &mut Context| {
            let $data = {
                let $pre_ctx: &mut Context = &mut *$ctx;
                $pre
            };
            async move {
                let _ = &$data;
                $body
            }
            .boxed()
        })
    }};

    // 带有 pre 处理的分支
    (| $ctx:ident, $data:ident | $body:block, | $pre_ctx:ident | $pre:block) => {{
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::Context;
        use $crate::http::types::Executor;

        let executor: Arc<Executor> = Arc::new(move |$ctx: &mut Context| {
            let $data = {
                let $pre_ctx: &mut Context = &mut *$ctx;
                $pre
            };

            async move {
                let _ = &$data;
                $body
            }
            .boxed()
        });
        executor
    }};

    // 仅 body 的分支
    (| $ctx:ident | $body:block) => {{
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::Context;
        use $crate::http::types::Executor;

        let executor: Arc<Executor> =
            Arc::new(move |$ctx: &mut Context| async move { $body }.boxed());
        executor
    }};
}

#[macro_export]
macro_rules! validator {
    ($($key:ident => $dsl:expr),* $(,)?) => {
        {
        use ahash::AHashMap;
        use std::sync::Arc;
        use $crate::http::middlewares::validator::to_validator;
        use $crate::http::types::Executor;

        let mut dsl_map: AHashMap<String, String> = AHashMap::new();

        $(
            dsl_map.insert(stringify!($key).to_string(), $dsl.to_string());
        )*

        let mw: Arc<Executor> = to_validator(dsl_map);
        mw
        }
    };
}

// 文件：src/macros.rs

#[macro_export]
macro_rules! v {
    ($($tokens:tt)*) => {
        $crate::validator!($($tokens)*)
    };
}
