use std::sync::Arc;
use crate::types::{HTTPContext, Executor};
use futures::FutureExt; // for `.boxed()`

// -----------------------------
// 通用方法宏生成器（内部使用）
// -----------------------------
#[macro_export]
macro_rules! make_method_macro {
    ($method_str:expr, $path:expr, $handler:expr $(, $middleware:expr)?) => {{
        use std::sync::Arc;
        use $crate::types::{HTTPContext, Executor};

        let handler_arc: Arc<Executor> = Arc::new($handler);

        let mw_arc_opt: Option<Vec<Arc<Executor>>> = None $(.or(Some(
            $middleware.into_iter()
                .map(|mw| Arc::new(mw) as Arc<Executor>)
                .collect::<Vec<_>>()
        )))?;

        ($method_str, $path, handler_arc, mw_arc_opt)
    }};
}


// -----------------------------
// HTTP 方法宏
// -----------------------------
#[macro_export]
macro_rules! get {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("GET", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! post {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("POST", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! put {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("PUT", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! delete {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("DELETE", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! patch {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("PATCH", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! options {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("OPTIONS", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! head {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("HEAD", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! trace {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("TRACE", $path, $handler $(, $middleware)?)
    };
}

#[macro_export]
macro_rules! connect {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("CONNECT", $path, $handler $(, $middleware)?)
    };
}

// -----------------------------
// 全局 all! 宏
// -----------------------------
#[macro_export]
macro_rules! all {
    ($path:expr, $handler:expr $(, $middleware:expr)?) => {
        $crate::make_method_macro!("*", $path, $handler $(, $middleware)?)
    };
}

// -----------------------------
// route! 宏
// -----------------------------
#[macro_export]
macro_rules! route {
    ($root:expr, $method_macro:expr) => {{
        let (method, path, handler, middleware) = $method_macro;
        $root.insert(
            path,
            if method == "*" { None } else { Some(method) },
            handler,
            middleware,
        );
    }};
}
