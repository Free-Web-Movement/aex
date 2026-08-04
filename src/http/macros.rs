//! # HTTP Macros
//!
//! Macros for defining HTTP handlers and middleware.
//!
//! ## Usage
//!
//! `exe!` builds an `Arc<Executor>` with an async body. Use it when a handler
//! or middleware needs to `await` inside the body (plain sync closures work
//! directly on routes: `router.get("/", |ctx| { ...; true })`).
//!
//! ```rust,ignore
//! use aex::exe;
//!
//! let handler = exe!(|ctx| {
//!     ctx.send("response");
//!     true
//! });
//! ```

#[macro_export]
#[allow(unused_variables)]
macro_rules! exe {
    // 支持 move 闭包
    (move | $ctx:ident | $body:block) => {{
        #[allow(unused_variables)]
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |$ctx: &mut Context| async move { $body }.boxed());
            executor
        }
    }};

    // 支持 move 闭包 + pre 处理
    (move | $ctx:ident, $data:ident | $body:block, | $pre_ctx:ident | $pre:block) => {{
        #[allow(unused_variables)]
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |$ctx: &mut Context| {
                    let _ = $ctx;
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
        }
    }};

    // 带有 pre 处理的分支
    (| $ctx:ident, $data:ident | $body:block, | $pre_ctx:ident | $pre:block) => {{
        #[allow(unused_variables)]
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |$ctx: &mut Context| {
                    let _ = $ctx;
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
        }
    }};

    // 仅 body 的分支
    (| $ctx:ident | $body:block) => {{
        #[allow(unused_variables)]
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |$ctx: &mut Context| async move { $body }.boxed());
            executor
        }
    }};
}

#[macro_export]
macro_rules! validator {
    ($($key:ident => $dsl:expr),* $(,)?) => {
        {
        use ahash::AHashMap;
        use std::sync::Arc;
        use $crate::http::middlewares::validator::to_validator;

        #[allow(unused_imports)]
        use $crate::http::types::Executor;

        let mut dsl_map: AHashMap<String, String> = AHashMap::new();

        $(
            dsl_map.insert(stringify!($key).to_string(), $dsl.to_string());
        )*

        let mw: std::sync::Arc<$crate::http::types::Executor> = to_validator(dsl_map);
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
