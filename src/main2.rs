use tokio::net::TcpListener;
use tokio::io::AsyncWriteExt;

static RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Length: 12\r\n\
Connection: close\r\n\
\r\n\
Hello world!";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:9000").await?;

    loop {
        let (mut stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            // 只写，不 shutdown，不 read
            let _ = stream.write_all(RESPONSE).await;
            // 作用域结束，stream 直接 drop → 内核 close
        });
    }
}
