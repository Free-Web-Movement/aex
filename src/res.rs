use std::{ collections::HashMap, path::Path };
use tokio::{ fs::File, io::{ AsyncReadExt, AsyncWriteExt, BufWriter } };
use crate::protocol::{ header::HeaderKey, media_type::MediaType };
use crate::protocol::status::StatusCode;
use serde::Serialize;
use tokio::net::tcp::OwnedWriteHalf;

const HTTP_BUFFER: usize = 8 * 1024;

/// HTTP 响应结构
pub struct Response {
    pub status: StatusCode,
    pub writer: BufWriter<OwnedWriteHalf>,
    pub headers: HashMap<HeaderKey, String>,
    pub body: Vec<String>,
    // peer_addr: SocketAddr,
}

impl Response {
    pub fn new(writer: BufWriter<OwnedWriteHalf>) -> Self {
        Response { status: StatusCode::Ok, writer, headers: HashMap::new(), body: vec![] }
    }

    /// 实例方法：将当前 Response 结构体中的所有内容一次性发送
    pub async fn send(&mut self) -> std::io::Result<()> {
        // 将 Vec<String> 类型的 body 拼接成一个字节数组
        let full_body = self.body.join("").into_bytes();

        // 将 HashMap<HeaderKey, String> 转换为 send_inner 要求的 HashMap<String, String>
        let mut string_headers = HashMap::with_capacity(self.headers.len());
        for (k, v) in &self.headers {
            string_headers.insert(k.to_str().to_string(), v.clone());
        }

        // 调用内部现有的核心发送逻辑
        Self::send_inner(&mut self.writer, self.status, string_headers, &full_body).await
    }

    // /// 直接写入字符串（不封装 HTTP 响应）
    // pub async fn write_str<S: AsRef<str>>(
    //     writer: &mut BufWriter<OwnedWriteHalf>,
    //     s: S
    // ) -> std::io::Result<()> {
    //     writer.write_all(s.as_ref().as_bytes()).await?;
    //     writer.flush().await
    // }

    // /// 直接写入字节（不封装 HTTP 响应）
    // pub async fn write_bytes(
    //     writer: &mut BufWriter<OwnedWriteHalf>,
    //     bytes: &[u8]
    // ) -> std::io::Result<()> {
    //     writer.write_all(bytes).await?;
    //     writer.flush().await
    // }

    /// 核心发送方法，内部统一处理 header 拼接、Content-Length 和 flush
    async fn send_inner(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        mut headers: HashMap<String, String>,
        body: &[u8]
    ) -> std::io::Result<()> {
        // 自动设置 Content-Length（除非已经设置）
        headers
            .entry(HeaderKey::ContentLength.to_str().to_string())
            .or_insert(body.len().to_string());

        // 构造响应头
        let mut response = format!("HTTP/1.1 {} {}\r\n", status as u16, status.to_str());
        for (key, value) in &headers {
            response.push_str(&format!("{}: {}\r\n", key, value));
        }
        response.push_str("\r\n");

        // 写入 header + body
        writer.write_all(response.as_bytes()).await?;
        writer.write_all(body).await?;
        writer.flush().await
    }

    /// 发送任意字节 body
    pub async fn send_bytes(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        headers: HashMap<String, String>,
        body: &[u8]
    ) -> std::io::Result<()> {
        Self::send_inner(writer, status, headers, body).await
    }

    /// 发送字符串 body
    pub async fn send_str<S: AsRef<str>>(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        headers: HashMap<String, String>,
        body: S
    ) -> std::io::Result<()> {
        Self::send_inner(writer, status, headers, body.as_ref().as_bytes()).await
    }

    /// 发送 JSON 数据（自动设置 Content-Type: application/json）
    pub async fn send_json<T: Serialize>(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        mut headers: HashMap<String, String>,
        value: &T
    ) -> std::io::Result<()> {
        // 序列化 JSON
        let json_body = serde_json
            ::to_vec(value)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("JSON serialize error: {}", e)
                )
            })?;

        // 设置 Content-Type: application/json
        headers
            .entry(HeaderKey::ContentType.to_str().to_string())
            .or_insert("application/json".to_string());

        Self::send_inner(writer, status, headers, &json_body).await
    }

    /// 只发送状态码，body 为空（Content-Length: 0）
    pub async fn send_status(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        mut headers: Option<HashMap<String, String>>
    ) -> std::io::Result<()> {
        let mut headers = headers.take().unwrap_or_default();
        headers.entry(HeaderKey::ContentLength.to_str().to_string()).or_insert("0".to_string());
        Self::send_inner(writer, status, headers, &[]).await
    }

    /// 发送本地文件
    pub async fn send_file<P: AsRef<Path>>(
        writer: &mut BufWriter<OwnedWriteHalf>,
        status: StatusCode,
        mut headers: HashMap<String, String>,
        file_path: P
    ) -> std::io::Result<()> {
        let path = file_path.as_ref();

        // 打开文件
        let mut file = File::open(path).await?;
        let file_size = file.metadata().await?.len();

        // 设置 Content-Length
        headers
            .entry(HeaderKey::ContentLength.to_str().to_string())
            .or_insert(file_size.to_string());

        // 设置 Content-Type（如果没设置的话）
        headers
            .entry(HeaderKey::ContentType.to_str().to_string())
            .or_insert(MediaType::guess(path).to_string());

        // 发送响应头
        let mut head = format!("HTTP/1.1 {}\r\n", status as usize);
        for (k, v) in &headers {
            head.push_str(&format!("{}: {}\r\n", k, v));
        }
        head.push_str("\r\n");
        writer.write_all(head.as_bytes()).await?;

        // 分块读取文件并写入
        let mut buffer = [0u8; HTTP_BUFFER]; // 8KB 缓冲
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break; // EOF
            }
            writer.write_all(&buffer[..n]).await?;
        }

        writer.flush().await
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{ TcpListener, TcpStream };
    use tokio::io::{ AsyncReadExt, BufWriter };
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs::write;
    use std::path::PathBuf;

    async fn spawn_response_server<F>(handler: F) -> String
    where
        F: FnOnce(BufWriter<OwnedWriteHalf>) -> tokio::task::JoinHandle<()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (_, write_half) = socket.into_split();
            let writer = BufWriter::new(write_half);
            handler(writer).await;
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        server.await.unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }

    #[tokio::test]
    async fn test_send_str() {
        let resp = spawn_response_server(|mut writer| {
            tokio::spawn(async move {
                Response::send_str(
                    &mut writer,
                    StatusCode::Ok,
                    HashMap::new(),
                    "hi"
                )
                .await
                .unwrap();
            })
        })
        .await;

        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Length: 2"));
        assert!(resp.ends_with("\r\n\r\nhi"));
    }

    #[tokio::test]
    async fn test_send_bytes() {
        let resp = spawn_response_server(|mut writer| {
            tokio::spawn(async move {
                Response::send_bytes(
                    &mut writer,
                    StatusCode::Created,
                    HashMap::new(),
                    b"bin"
                )
                .await
                .unwrap();
            })
        })
        .await;

        assert!(resp.contains("201 Created"));
        assert!(resp.ends_with("\r\n\r\nbin"));
    }

    #[tokio::test]
    async fn test_send_json() {
        let resp = spawn_response_server(|mut writer| {
            tokio::spawn(async move {
                Response::send_json(
                    &mut writer,
                    StatusCode::Ok,
                    HashMap::new(),
                    &json!({ "a": 1 })
                )
                .await
                .unwrap();
            })
        })
        .await;

        assert!(resp.contains("Content-Type: application/json"));
        assert!(resp.ends_with("\r\n\r\n{\"a\":1}"));
    }

    #[tokio::test]
    async fn test_send_status() {
        let resp = spawn_response_server(|mut writer| {
            tokio::spawn(async move {
                Response::send_status(
                    &mut writer,
                    StatusCode::NoContent,
                    None
                )
                .await
                .unwrap();
            })
        })
        .await;

        assert!(resp.contains("204 No Content"));
        assert!(resp.contains("Content-Length: 0"));
    }

    #[tokio::test]
    async fn test_response_send() {
        let resp = spawn_response_server(|mut writer| {
            tokio::spawn(async move {
                let mut res = Response::new(writer);
                res.status = StatusCode::Accepted;
                res.headers.insert(
                    HeaderKey::ContentType,
                    "text/plain".into()
                );
                res.body.push("ok".into());
                res.send().await.unwrap();
            })
        })
        .await;

        assert!(resp.contains("202 Accepted"));
        assert!(resp.contains("Content-Type: text/plain"));
        assert!(resp.ends_with("\r\n\r\nok"));
    }

    #[tokio::test]
    async fn test_send_file() {
        let tmp = PathBuf::from("test_response_file.txt");
        let cloned = tmp.clone();
        write(&tmp, "FILE").unwrap();

        let resp = spawn_response_server(move |mut writer| {
            let path = tmp.clone();
            tokio::spawn(async move {
                Response::send_file(
                    &mut writer,
                    StatusCode::Ok,
                    HashMap::new(),
                    path
                )
                .await
                .unwrap();
            })
        })
        .await;

        assert!(resp.contains("Content-Length: 4"));
        assert!(resp.contains("Content-Type: text/plain"));
        assert!(resp.ends_with("FILE"));

        let _ = std::fs::remove_file(cloned);
    }
}
