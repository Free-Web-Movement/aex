// use std::net::SocketAddr;
// use std::sync::Arc;
// use tokio::net::TcpListener;
// use tokio::sync::Mutex;
// use aex::tcp::types::{RawCodec, Codec, Command};
// use aex::connection::global::GlobalContext;
// use aex::connection::context::{Context, TypeMapExt};
// use aex::connection::manager::ConnectionManager;
// use aex::tcp::router::Router as TcpRouter;
// use aex::connection::types::IDExtractor;
// use aex::connection::entry::ConnectionEntry;

// #[tokio::test]
// async fn test_bidirectional_p2p_stress() {
//     // 1. 设置服务端
//     let addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
//     let mut server_router = TcpRouter::new();
//     let (tx, mut rx) = tokio::sync::mpsc::channel(100);

//     server_router.on::<RawCodec, RawCodec>(
//         1001,
//         Box::new(move |ctx, _frame, cmd| {
//             let tx = tx.clone();
//             Box::pin(async move {
//                 // 回显逻辑：服务端收到后立即回传给客户端
//                 let response = cmd.clone();
//                 let mut guard = ctx.lock().await;
//                 if let Some(w) = guard.writer.as_mut() {
//                     use tokio::io::AsyncWriteExt;
//                     w.write_all(&response.encode()).await.unwrap();
//                 }
//                 let _ = tx.send(cmd.0).await;
//                 Ok(true)
//             })
//         }),
//         vec![],
//     );

//     let server_global = Arc::new(GlobalContext::new(addr, None));
//     server_global.routers.set_value(Arc::new(server_router));

//     let listener = TcpListener::bind(addr).await.unwrap();
//     tokio::spawn(async move {
//         loop {
//             let (socket, peer) = listener.accept().await.unwrap();
//             let global = server_global.clone();
//             let extractor: IDExtractor<RawCodec> = Arc::new(|c: &RawCodec| c.id());
//             let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(peer, true, extractor);
//             let (token, _handle, _) = ConnectionEntry::start::<RawCodec, RawCodec, _, _>(
//                 global.manager.cancel_token.clone(),
//                 socket,
//                 peer,
//                 global.clone(),
//                 {
//                     let global = global.clone();
//                     let peer = peer;
//                     move |ctx| {
//                         let global_inner = global.clone();
//                         let peer_inner = peer;
//                         Box::pin(async move {
//                              global_inner.manager.update(peer_inner, true, Some(ctx.clone()));
//                              let _ = pipeline(ctx).await;
//                              Ok(())
//                         })
//                     }
//                 },
//             );
//             global.manager.add(peer, _handle, token, true, None);
//         }
//     });

//     // 2. 设置客户端
//     let manager = ConnectionManager::new();
//     let client_global = Arc::new(GlobalContext::new(addr, None));
//     let extractor: IDExtractor<RawCodec> = Arc::new(|c: &RawCodec| c.id());

//     manager.connect::<RawCodec, RawCodec, _, _>(
//         addr,
//         client_global,
//         |_ctx| async move {},
//         extractor,
//     ).await.unwrap();

//     // 💡 修复：连接建立是异步的，需要短暂等待 Manager 登记
//     tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
//     println!("Manager connections size: {}", manager.connections.len());
//     for b in manager.connections.iter() {
//         println!("Bucket {:?}: clients={}, servers={}", b.key(), b.value().clients.len(), b.value().servers.len());
//     }

//     // 3. 双向同时读写测试
//     let mut client_entry = None;
//     for bucket_ref in manager.connections.iter() {
//         println!("Bucket {:?}: clients={}, servers={}", bucket_ref.key(), bucket_ref.value().clients.len(), bucket_ref.value().servers.len());
//         if !bucket_ref.value().clients.is_empty() {
//              client_entry = Some(bucket_ref.value().clients.iter().next().unwrap().value().clone());
//              break;
//         }
//         if !bucket_ref.value().servers.is_empty() {
//              client_entry = Some(bucket_ref.value().servers.iter().next().unwrap().value().clone());
//              break;
//         }
//     }
//     let client_entry = client_entry.expect("No connection found");
//     let ctx = client_entry.context.clone().expect("Context should not be None");

//     for i in 0..50 {
//         let cmd = RawCodec(vec![0, 0, 3, 233, i]); // ID 1001
//         {
//             let mut guard = ctx.lock().await;
//             if let Some(w) = guard.writer.as_mut() {
//                 use tokio::io::AsyncWriteExt;
//                 w.write_all(&cmd.encode()).await.unwrap();
//             }
//         }
//         // 验证回显
//         assert_eq!(rx.recv().await.unwrap(), cmd.0);
//     }
// }
