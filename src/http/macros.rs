//! # HTTP Macros
//!
//! Macros for defining HTTP handlers and middleware.
//!
//! ## Usage
//!
//! `_async!` builds an `Arc<Executor>` with an async body. Use it when a handler
//! or middleware needs to `await` inside the body. 参数可写 `|ctx|`，也可写通配
//! `|_|`（body 内不引用 ctx 时）：`router.get("/", _async!(|_| { true }))`。
//!
//! `_sync!` builds an `Arc<Executor>` from a plain sync closure and restores the
//! un-annotated form: `router.get("/", _sync!(|_| "OK"))`.
//!
//! ```rust,ignore
//! use aex::_async;
//!
//! let handler = _async!(|ctx| {
//!     ctx.send("response");
//!     true
//! });
//! ```

#[macro_export]
#[allow(unused_variables)]
macro_rules! _async {
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

    // 通配参数：`|_| { ... }`（body 内不能引用 ctx）
    (|_| $body:block) => {{
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |_: &mut Context| async move { $body }.boxed());
            executor
        }
    }};

    // move 闭包 + 通配参数
    (move |_| $body:block) => {{
        {
            use std::sync::Arc;
            use $crate::FutureExt;
            use $crate::connection::context::Context;

            #[allow(unused_imports)]
            use $crate::http::types::Executor;

            let executor: std::sync::Arc<$crate::http::types::Executor> =
                Arc::new(move |_: &mut Context| async move { $body }.boxed());
            executor
        }
    }};
}

/// `exe!` 是 `_async!` 的别名，保留兼容旧写法。
#[macro_export]
macro_rules! exe {
    ($($tt:tt)*) => {
        $crate::_async!($($tt)*)
    };
}

/// `then!` 是 `_async!` 的别名（then = 稍后异步执行）。
#[macro_export]
macro_rules! then {
    ($($tt:tt)*) => {
        $crate::_async!($($tt)*)
    };
}

/// `now!` 是 `_sync!` 的别名（now = 立即同步执行）。
#[macro_export]
macro_rules! now {
    ($($tt:tt)*) => {
        $crate::_sync!($($tt)*)
    };
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
    ($($tokens:tt)*) => {        $crate::validator!($($tokens)*)
    };
}

/// `_sync!` builds an `Arc<Executor>` from a plain (non-async) closure while
/// keeping the ergonomic un-annotated form: the generic `_sync` function infers
/// the closure's parameter type, so `_sync!(|_| "OK")` works instead of
/// `|_ctx: &mut Context| "OK"`.
///
/// ```rust
/// use aex::http::router::Router as HttpRouter;
/// use aex::_sync;
///
/// let mut router = HttpRouter::default();
/// router.get("/", _sync!(|_| "OK"));
/// ```
#[macro_export]
macro_rules! _sync {
    ($f:expr) => {
        $crate::http::types::sync($f)
    };
}
