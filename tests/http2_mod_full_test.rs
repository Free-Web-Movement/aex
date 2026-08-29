use aex::http2::H2Codec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn h2_is_h2_connection_true_for_preface() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(H2Codec::is_h2_connection(&mut server).await);
}

#[tokio::test]
async fn h2_is_h2_connection_false_for_http11() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(!H2Codec::is_h2_connection(&mut server).await);
}

#[tokio::test]
async fn h2_is_h2_connection_false_for_short_data() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"PRI * ").await.unwrap();
    client.flush().await.unwrap();
    let (mut server, _) = listener.accept().await.unwrap();
    assert!(!H2Codec::is_h2_connection(&mut server).await);
}
