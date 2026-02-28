// for `.boxed()`

// -----------------------------
// 通用方法宏生成器（内部使用）
// -----------------------------
#[macro_export]
macro_rules! make_method_macro {
    ($method_str:expr, $path:expr, $handler:expr $(, $middleware:expr)?) => {
        {
            use std::sync::Arc;
            use $crate::http::types::{Executor};

            // 修正：假设 $handler 已经是 Arc<Executor> 类型
            let handler_arc: Arc<Executor> = $handler;

            // 修正：中间件列表处理，支持直接传入 Vec<Arc<Executor>>
            let mw_arc_opt: Option<Vec<Arc<Executor>>> = None $(.or(Some(
                $middleware // 这里直接使用传入的 Vec，因为 exe! 已经包好了 Arc
            )))?;

            ($method_str, $path, handler_arc, mw_arc_opt)
        }
    };
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
    ($root:expr, $method_macro:expr) => {
        {
        let (method, path, handler, middleware) = $method_macro;
        $root.insert(
            path,
            if method == "*" { None } else { Some(method) },
            handler,
            middleware,
        );
        }
    };
}

#[macro_export]
macro_rules! exe {
    // 带有 pre 处理的分支
    (| $ctx:ident, $data:ident | $body:block, | $pre_ctx:ident | $pre:block) => {
        {
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::HTTPContext;
        use $crate::http::types::Executor;

        // 显式指定闭包的生命周期约束
        let executor: Arc<Executor> = Arc::new(move |$ctx: &mut HTTPContext| {
            // 1. 同步执行 pre
            let $data = {
                let $pre_ctx: &mut HTTPContext = &mut *$ctx;
                $pre
            };

            // 2. 将异步块包装并显式绑定生命周期
            async move {
                let _ = &$data; // 强制捕获 data
                $body
            }
            .boxed() // 相当于 Box::pin(async move { ... })
        });
        executor
        }
    };

    // 仅 body 的分支
    (| $ctx:ident | $body:block) => {
        {
        use futures::future::FutureExt;
        use std::sync::Arc;
        use $crate::connection::context::HTTPContext;
        use $crate::http::types::Executor;

        let executor: Arc<Executor> =
            Arc::new(move |$ctx: &mut HTTPContext| async move { $body }.boxed());
        executor
        }
    };
}

#[macro_export]
macro_rules! validator {
    ($($key:ident => $dsl:expr),* $(,)?) => {
        {
        use std::collections::HashMap;
        use std::sync::Arc;
        use $crate::http::middlewares::validator::to_validator;
        use $crate::http::types::Executor;

        let mut dsl_map: HashMap<String, String> = HashMap::new();

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

#[macro_export]
macro_rules! body {
    // 基础模式
    ($ctx:expr, $content:expr) => {
        {
            use $crate::http::protocol::header::HeaderKey;
            let mut meta = $ctx.local.get_value::<HttpMetadata>().unwrap();

            // 修复点：显式转为 Vec<u8>，消除 into() 的歧义
            let bytes: Vec<u8> = $content.as_bytes().to_vec();
            let len = bytes.len();

            // 自动插入到 Headers 集合中
            meta.headers.insert(
                HeaderKey::ContentLength, 
                len.to_string()
            );
            
            // 赋值 Body
            meta.body = bytes;
            $ctx.local.set_value::<HttpMetadata>(meta);
        }
    };

    // 扩展模式
    ($ctx:expr, $content:expr, { $($key:expr => $val:expr),* $(,)? }) => {
        {
            // 注意：在调用自身时，最好使用 $crate::body 以防作用域冲突
            $(
                $ctx.meta.headers.insert($key, $val.into());
            )*     
            $crate::body!($ctx, $content); 

        }
    };
}
