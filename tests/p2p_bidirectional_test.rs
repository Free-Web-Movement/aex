use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::manager::ConnectionManager;
use aex::connection::node::Node;
use aex::connection::entry::ConnectionEntry;
use aex::tcp::types::{RawCodec, Codec, Command, TCPCommand, TCPFrame};
use tokio_util::sync::CancellationToken;

fn create_global(addr: SocketAddr) -> Arc<GlobalContext> {
    use aex::crypto::session_key_manager::PairedSessionKey;
    use tokio::sync::Mutex;
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    Arc::new(GlobalContext::new(addr, Some(keys)))
}

#[tokio::test]
async fn test_p2p_bidirectional_basics() {
    let addr_a: SocketAddr = "127.0.0.1:19001".parse().unwrap();
    let addr_b: SocketAddr = "127.0.0.1:19002".parse().unwrap();

    let (tx_a, mut rx_a) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (tx_b, mut rx_b) = tokio::sync::mpsc::channel::<Vec<u8>>(100);

    let global_a = create_global(addr_a);
    let global_b = create_global(addr_b);

    let manager_a = Arc::new(ConnectionManager::new());
    let manager_b = Arc::new(ConnectionManager::new());

    let listener_a = TcpListener::bind(addr_a).await.unwrap();
    let listener_b = TcpListener::bind(addr_b).await.unwrap();

    let tx_a_clone = tx_a.clone();
    let global_a_clone = global_a.clone();
    let manager_a_clone = manager_a.clone();
    tokio::spawn(async move {
        let (socket, peer) = listener_a.accept().await.unwrap();
        
        let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(peer, true);
        
        let (token, handle, ctx) = ConnectionEntry::start::<RawCodec, RawCodec, _, _>(
            manager_a_clone.cancel_token.clone(),
            socket,
            peer,
            global_a_clone.clone(),
            move |ctx| {
                Box::pin(async move {
                    let _ = pipeline(ctx.clone()).await;
                    
                    let mut guard = ctx.lock().await;
                    let reader = guard.reader.take();
                    if let Some(mut r) = reader {
                        let mut buf = vec![0u8; 1024];
                        loop {
                            match r.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    let data = buf[..n].to_vec();
                                    let _ = tx_a_clone.send(data).await;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Ok(())
                })
            },
        );
        manager_a_clone.add(peer, handle, token, true, Some(ctx));
    });

    let tx_b_clone = tx_b.clone();
    let global_b_clone = global_b.clone();
    let manager_b_clone = manager_b.clone();
    tokio::spawn(async move {
        let (socket, peer) = listener_b.accept().await.unwrap();
        
        let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(peer, true);
        
        let (token, handle, ctx) = ConnectionEntry::start::<RawCodec, RawCodec, _, _>(
            manager_b_clone.cancel_token.clone(),
            socket,
            peer,
            global_b_clone.clone(),
            move |ctx| {
                Box::pin(async move {
                    let _ = pipeline(ctx.clone()).await;
                    
                    let mut guard = ctx.lock().await;
                    let reader = guard.reader.take();
                    if let Some(mut r) = reader {
                        let mut buf = vec![0u8; 1024];
                        loop {
                            match r.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    let data = buf[..n].to_vec();
                                    let _ = tx_b_clone.send(data).await;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Ok(())
                })
            },
        );
        manager_b_clone.add(peer, handle, token, true, Some(ctx));
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let socket_ab = tokio::net::TcpStream::connect(addr_a).await.unwrap();
    let (read_a, mut write_a) = socket_ab.into_split();
    
    let socket_ba = tokio::net::TcpStream::connect(addr_b).await.unwrap();
    let (read_b, mut write_b) = socket_ba.into_split();

    let msg_from_a = b"Message from node A to node B";
    let msg_from_b = b"Message from node B to node A";

    write_a.write_all(msg_from_a).await.unwrap();
    write_b.write_all(msg_from_b).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let received_by_b = rx_a.recv().await.unwrap();
    let received_by_a = rx_b.recv().await.unwrap();

    assert_eq!(received_by_b, msg_from_a);
    assert_eq!(received_by_a, msg_from_b);
    
    drop(read_a);
    drop(read_b);
}

#[tokio::test]
async fn test_p2p_bidirectional_stress() {
    let server_addr: SocketAddr = "127.0.0.1:19501".parse().unwrap();
    
    let global = create_global(server_addr);
    let manager = Arc::new(ConnectionManager::new());

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(SocketAddr, Vec<u8>)>(500);
    let listener = TcpListener::bind(server_addr).await.unwrap();
    
    let global_clone = global.clone();
    let manager_clone = manager.clone();
    let tx_clone = tx.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    if let Ok((socket, peer)) = result {
                        let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(peer, true);
                        let tx_inner = tx_clone.clone();
                        
                        let (token, handle, ctx) = ConnectionEntry::start::<RawCodec, RawCodec, _, _>(
                            manager_clone.cancel_token.clone(),
                            socket,
                            peer,
                            global_clone.clone(),
                            move |ctx| {
                                Box::pin(async move {
                                    let _ = pipeline(ctx.clone()).await;
                                    
                                    let mut guard = ctx.lock().await;
                                    let reader = guard.reader.take();
                                    if let Some(mut r) = reader {
                                        let mut buf = vec![0u8; 1024];
                                        loop {
                                            match r.read(&mut buf).await {
                                                Ok(0) => break,
                                                Ok(n) => {
                                                    let data = buf[..n].to_vec();
                                                    let _ = tx_inner.send((peer, data)).await;
                                                }
                                                Err(_) => break,
                                            }
                                        }
                                    }
                                    Ok(())
                                })
                            },
                        );
                        manager_clone.add(peer, handle, token, true, Some(ctx));
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    break;
                }
            }
        }
    });

    let mut handles = vec![];
    for i in 0..5 {
        let addr = server_addr;
        let global = global.clone();
        
        let socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = socket.into_split();
        
        let msg = format!("Client {} message", i);
        writer.write_all(msg.as_bytes()).await.unwrap();
        
        handles.push((reader, writer));
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut received_count = 0;
    for _ in 0..5 {
        if rx.recv().await.is_some() {
            received_count += 1;
        }
    }

    assert!(received_count >= 3, "Should receive at least 3 messages");
}

#[tokio::test]
async fn test_p2p_node_entry_basic() {
    let node = Node::from_system(8080, vec![0x11u8; 32], 1);
    
    assert_eq!(node.id, vec![0x11u8; 32]);
    assert_eq!(node.port, 8080);
    assert_eq!(node.version, 1);
    
    let ips = node.get_all();
    assert!(!ips.is_empty());
}

#[tokio::test]
async fn test_p2p_network_scope_classification() {
    use aex::connection::scope::NetworkScope;
    
    let intranet_ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();
    let extranet_ip: std::net::IpAddr = "8.8.8.8".parse().unwrap();
    
    let scope_intranet = NetworkScope::from_ip(&intranet_ip);
    let scope_extranet = NetworkScope::from_ip(&extranet_ip);
    
    assert_eq!(scope_intranet, NetworkScope::Intranet);
    assert_eq!(scope_extranet, NetworkScope::Extranet);
}