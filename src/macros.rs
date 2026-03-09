#[macro_export]
macro_rules! on {
    // 递归出口：当列表处理完毕
    ($router:expr, $frame_type:ty, $cmd_type:ty, []) => {};

    // 核心匹配模式：[ action, handler, [middlewares] ]
    ($router:expr, $frame_type:ty, $cmd_type:ty, [ 
        [ $action:expr, $handler:ident, [$($ms:expr),*] ] $(, $($rest:tt)*)? 
    ]) => {
        $router.on::<$frame_type, $cmd_type>(
            $action,
            Box::new(|ctx, frame, cmd| {
                Box::pin(async move {
                    $handler(ctx, frame, cmd).await;
                    Ok(true)
                })
            }),
            vec![
                $(
                    Box::new(|ctx, frame, cmd| {
                        Box::pin(async move {
                            $ms(ctx, frame, cmd).await
                        })
                    })
                ),*
            ]
        );

        // 递归处理后续项
        $crate::on!($router, $frame_type, $cmd_type, [ $($($rest)*)? ]);
    };

    // 便捷模式：支持省略中间件的简写 [ action, handler ]
    ($router:expr, $frame_type:ty, $cmd_type:ty, [ 
        [ $action:expr, $handler:ident ] $(, $($rest:tt)*)? 
    ]) => {
        $crate::on!($router, $frame_type, $cmd_type, [ [ $action, $handler, [] ] $(, $($rest)*)? ]);
    };
}