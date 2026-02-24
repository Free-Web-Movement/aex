// #[cfg(test)]
// mod tests {
//     use super::*;
//     use std::{net::SocketAddr, sync::Arc};
//     use aex::{connection::context::{Context, GlobalContext, HTTPContext, TypeMapExt}, http::types::{Executor, to_executor}};
//     use futures::FutureExt; // 必须引入以使用 .boxed()

//     #[tokio::test]
//     async fn test_to_executor_with_context_views() {
//         // 1. 初始化基础环境
//         let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
//         let global = Arc::new(GlobalContext::new(addr));
        
//         // 2. 使用 to_executor 包装一个复杂的异步业务逻辑
//         let executor = to_executor(|ctx| {
//             async move {
//                 // 在执行器内部设置 Context 的 local 数据
//                 ctx.local.set_value("auth_token".to_string());

//                 // 测试：在 Executor 内部构造 Request 视图并访问数据
//                 let req_view = ctx.req().await;
//                 let has_token = req_view.local.get_value::<String>().is_some();

//                 // 测试：在 Executor 内部构造 Response 视图并操作 I/O
//                 let res_view = ctx.res();
//                 let mut writer_lock = res_view.writer.lock().await;
//                 writer_lock.extend_from_slice(b"executed");

//                 has_token && writer_lock.len() > 0
//             }.boxed()
//         });

//         // 3. 构造真实的 Context 实例
//         let reader = Vec::new();
//         let writer = Vec::new();
//         let mut ctx = HTTPContext::<BufWriter<OwnedWriteHalf>>::new(reader, writer, global.clone(), addr);

//         // 4. 运行 executor 并验证结果
//         let result = executor(&mut ctx).await;

//         // 5. 断言
//         assert!(result, "Executor 逻辑应返回 true");
//         assert_eq!(ctx.local.get_value::<String>(), Some("auth_token".to_string()));
        
//         // 验证 Writer 是否真的被 Executor 修改了
//         let final_writer = ctx.writer.lock().await;
//         assert!(final_writer.ends_with(b"executed"));
//     }

//     #[tokio::test]
//     async fn test_executor_chaining_simulation() {
//         // 验证 Executor 是否可以被存入集合并在循环中异步执行
//         let mut pipeline: Vec<Arc<Executor>> = Vec::new();

//         pipeline.push(to_executor(|_| async { true }.boxed()));
//         pipeline.push(to_executor(|ctx| {
//             async move {
//                 ctx.addr.is_ipv4()
//             }.boxed()
//         }));

//         let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
//         let mut ctx = Context::new(vec![], vec![], Arc::new(GlobalContext::new(addr)), addr);

//         for exec in pipeline {
//             let ok = exec(&mut ctx).await;
//             assert!(ok);
//         }
//     }
// }