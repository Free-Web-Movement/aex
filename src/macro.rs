
// -----------------------------
// 所有方法列表宏
// -----------------------------
macro_rules! http_methods_list {
    ($macro:ident) => {
        $macro!(get, "GET");
        $macro!(post, "POST");
        $macro!(put, "PUT");
        $macro!(delete, "DELETE");
        $macro!(patch, "PATCH");
        $macro!(options, "OPTIONS");
        $macro!(head, "HEAD");
        $macro!(trace, "TRACE");
        $macro!(connect, "CONNECT");
    };
}
// -----------------------------
// handler 与 middleware 宏
// -----------------------------
macro_rules! handler {
    ($func:expr) => {
        {
        Arc::new(move |ctx: &mut HTTPContext| { ($func)(ctx).boxed() }) as Arc<Executor>
        }
    };
}

// middleware 可以写成 Some 或 None
macro_rules! middleware {
    ($func:expr) => {
        {
        Some(vec![Arc::new(Middleware::new(
            move |ctx: &mut HTTPContext| { ($func)(ctx).boxed() },
            move |ctx: &mut HTTPContext| { ($func)(ctx).boxed() },
        ))])
        }
    };
    () => {
        None
    };
}

// 生成方法宏
macro_rules! make_method_macro {
    ($name:ident, $method_str:expr) => {
        macro_rules! $name {
            // middleware 统一传 Option<vec>
            ($path:expr, $handler:expr, $middleware:expr) => {{
                ($method_str, $path, $handler, $middleware)
            }};
            ($path:expr, $handler:expr) => {{
                ($method_str, $path, $handler, None)
            }};
        }
    };
}

// 调用生成所有 HTTP 方法宏
http_methods_list!(make_method_macro);

// 全局 all! 宏
macro_rules! all {
    ($path:expr, $handler:expr, $middleware:expr) => {
        {
        ("*", $path, $handler, $middleware)
        }
    };
}

// route! 宏
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

