use tokio::io::{ AsyncBufReadExt, AsyncReadExt, BufReader };
use tokio::net::tcp::OwnedReadHalf;
use tokio::time::timeout;

use std::net::SocketAddr;
use std::collections::HashMap;
use std::time::Duration;

use anyhow::{ Context, bail };

use crate::params::Params;
use crate::protocol::content_type::ContentType;
use crate::protocol::header::HeaderKey;
use crate::protocol::method::HttpMethod;
use crate::protocol::media_type::MediaType;
use crate::websocket::WebSocket;

static MAX_CAPACITY: i32 = 1024;
static TIME_LIMIT: i32 = 500;

pub struct Request {
    pub method: HttpMethod,
    pub path: String,
    pub is_chunked: bool, // 是否使用 Transfer-Encoding: chunked
    pub transfer_encoding: Option<String>, // 保存 Transfer-Encoding header 原始值
    pub multipart_boundary: Option<String>, // multipart/form-data 的 boundary
    pub version: String,
    pub params: Params, // 动态 path params
    pub headers: HashMap<HeaderKey, String>,
    pub content_type: ContentType,
    pub body: Vec<u8>,
    pub cookies: HashMap<String, String>,
    pub reader: BufReader<OwnedReadHalf>,
    pub peer_addr: SocketAddr,
    pub is_websocket: bool,
}

impl Request {
    #[inline]
    pub async fn read_line_with_limit(
        reader: &mut BufReader<OwnedReadHalf>,
        time_limit: Duration,
        capacity_limit: usize
    ) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(capacity_limit);

        let n = timeout(time_limit, reader.read_until(b'\n', &mut buf)).await.map_err(|_|
            anyhow::anyhow!("read line timeout")
        )??;

        // 连接被关闭
        if n == 0 {
            bail!("connection closed");
        }

        // 单行长度限制
        if buf.len() > capacity_limit {
            bail!("line too long (>{} bytes)", capacity_limit);
        }

        Ok(buf)
    }
    #[inline]
    pub async fn read_first_line(
        reader: &mut BufReader<OwnedReadHalf>
    ) -> anyhow::Result<(String, String, String)> {
        let line = Self::read_line_with_limit(
            reader,
            Duration::new(TIME_LIMIT as u64, 0),
            MAX_CAPACITY as usize
        ).await?;

        let line_str = std::str::from_utf8(&line).context("request line is not valid UTF-8")?;

        Self::parse_request_line(line_str).ok_or_else(||
            anyhow::anyhow!("invalid HTTP request line")
        )
    }

    /// 专门解析 HTTP 请求行: "GET /index.html HTTP/1.1"
    #[inline]
    pub fn parse_request_line(line: &str) -> Option<(String, String, String)> {
        // let mut parts = line.split(" ");
        let mut parts = line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        Some((method, path, version))
    }

    pub async fn new(
        mut reader: BufReader<OwnedReadHalf>,
        peer_addr: SocketAddr,
        route_pattern: &str
    ) -> anyhow::Result<Self> {
        // 1️⃣ 首行
        // let first_line = Self::read_first_line(&mut reader).await?;
        let (method_str, path, version) = Self::read_first_line(&mut reader).await?;

        let method = HttpMethod::from_str(&method_str).ok_or_else(||
            anyhow::anyhow!("unsupported HTTP method: {}", method_str)
        )?;

        // 2️⃣ headers
        let headers = Self::read_headers(&mut reader).await?;

        // 3️⃣ cookies
        let cookies = headers
            .get(&HeaderKey::Cookie)
            .map(|s| Self::parse_cookies_raw(s))
            .unwrap_or_default();

        // 4️⃣ Transfer-Encoding
        let (is_chunked, transfer_encoding) = if
            let Some(te) = headers.get(&HeaderKey::TransferEncoding)
        {
            (te.to_ascii_lowercase().contains("chunked"), Some(te.clone()))
        } else {
            (false, None)
        };

        // 5️⃣ Content-Length
        let length = headers
            .get(&HeaderKey::ContentLength)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);

        // 6️⃣ body
        let mut body = vec![0u8; length];
        if length > 0 {
            reader.read_exact(&mut body).await?;
        }

        // 7️⃣ content_type
        let content_type = headers
            .get(&HeaderKey::ContentType)
            .map(|s| ContentType::parse(s))
            .unwrap_or_else(|| ContentType::parse(""));

        let multipart_boundary = if
            content_type.top_level == MediaType::Multipart &&
            content_type.sub_type.eq_ignore_ascii_case("form-data")
        {
            content_type.parameters
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("boundary"))
                .map(|(_, v)| v.clone())
        } else {
            None
        };

        // 8️⃣ params
        let params = Params::new(path.clone(), route_pattern.to_string());

        let is_websocket = WebSocket::check(method, &headers);

        Ok(Request {
            method,
            path,
            version,
            headers,
            params,
            cookies,
            content_type,
            is_chunked,
            transfer_encoding,
            multipart_boundary,
            body,
            reader,
            peer_addr,
            is_websocket,
        })
    }

    /// 将 Cookie header 转换为 HashMap
    fn parse_cookies_raw(header_value: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for pair in header_value.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let mut kv = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        map
    }

    pub fn get_length(headers: &HashMap<HeaderKey, String>) -> usize {
        headers
            .get(&HeaderKey::ContentLength)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0)
    }

    /// 读取请求头并更新到 Request.headers 和 content_type
    pub async fn read_headers(
        reader: &mut BufReader<OwnedReadHalf>
    ) -> anyhow::Result<HashMap<HeaderKey, String>> {
        let mut headers_map = HashMap::new();
        let mut buf = String::new();

        loop {
            buf.clear();
            let line_bytes = Self::read_line_with_limit(
                reader,
                Duration::new(TIME_LIMIT as u64, 0),
                MAX_CAPACITY as usize
            ).await?;
            let line = std::str
                ::from_utf8(&line_bytes)
                .context("header line not valid UTF-8")?
                .trim_end_matches("\r\n");

            if line.is_empty() {
                break; // headers 结束
            }

            if let Some(pos) = line.find(':') {
                let key = &line[..pos].trim();
                let value = &line[pos + 1..].trim();
                if let Some(header_key) = HeaderKey::from_str(key) {
                    headers_map.insert(header_key, value.to_string());
                }
            }
        }

        Ok(headers_map)
    }

    pub async fn is_http_connection(reader: &mut OwnedReadHalf) -> anyhow::Result<bool> {
        let mut buf = [0u8; MAX_CAPACITY as usize];

        let n = reader.peek(&mut buf).await?;

        if n == 0 {
            return Ok(false);
        }

        let s = std::str::from_utf8(&buf[..n]).unwrap_or("");
        Ok(HttpMethod::is_prefixed(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::io::AsyncWriteExt;
    use std::net::SocketAddr;
    use std::collections::HashMap;
    use anyhow::Result;

    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

    async fn spawn_request_server(request_bytes: &[u8], route: &str) -> Request {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();

        // let patten = route.clone();
        let route = route.to_string();

        let server = tokio::spawn(async move {
            let (socket, peer_addr) = listener.accept().await.unwrap();
            let reader = BufReader::new(socket.into_split().0);
            Request::new(reader, peer_addr, &route).await.unwrap()
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(request_bytes).await.unwrap();

        server.await.unwrap()
    }
    #[tokio::test]
    async fn test_request_basic_get() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();

        // server task
        let server = tokio::spawn(async move {
            let (socket, peer_addr) = listener.accept().await.unwrap();
            let reader = BufReader::new(socket.into_split().0);
            Request::new(reader, peer_addr, "/").await.unwrap()
        });

        // client request
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request_str =
            "\
GET /hello HTTP/1.1\r\n\
Host: localhost\r\n\
Cookie: foo=bar; hello=world\r\n\
Content-Length: 5\r\n\
Content-Type: text/plain\r\n\
Transfer-Encoding: chunked\r\n\
\r\n\
abcde";
        client.write_all(request_str.as_bytes()).await.unwrap();

        let req = server.await.unwrap();

        assert_eq!(req.method, HttpMethod::GET);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.headers.get(&HeaderKey::Host).unwrap(), "localhost");
        assert_eq!(req.cookies.get("foo").unwrap(), "bar");
        assert_eq!(req.cookies.get("hello").unwrap(), "world");
        assert_eq!(req.body, b"abcde");
        assert!(req.is_chunked);
        assert_eq!(req.transfer_encoding.unwrap(), "chunked");
        assert_eq!(req.content_type.top_level, MediaType::Text);
        assert_eq!(req.content_type.sub_type, "plain");
        assert!(!req.is_websocket);
    }

    #[tokio::test]
    async fn test_request_post_multipart() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, peer_addr) = listener.accept().await.unwrap();
            let reader = BufReader::new(socket.into_split().0);
            Request::new(reader, peer_addr, "/upload").await.unwrap()
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
        let request_str =
            format!("\
POST /upload HTTP/1.1\r\n\
Host: localhost\r\n\
Content-Length: 4\r\n\
Content-Type: multipart/form-data; boundary={}\r\n\
\r\n\
abcd", boundary);

        client.write_all(request_str.as_bytes()).await.unwrap();

        let req = server.await.unwrap();

        assert_eq!(req.method, HttpMethod::POST);
        assert_eq!(req.path, "/upload");
        assert_eq!(req.multipart_boundary.unwrap(), boundary);
        assert_eq!(req.body, b"abcd");
        assert_eq!(req.content_type.top_level, MediaType::Multipart);
        assert_eq!(req.content_type.sub_type, "form-data");
    }

    #[tokio::test]
    async fn test_request_websocket_detection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, peer_addr) = listener.accept().await.unwrap();
            let reader = BufReader::new(socket.into_split().0);
            Request::new(reader, peer_addr, "/ws").await.unwrap()
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request_str =
            "\
GET /ws HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: abc123==\r\n\
Sec-WebSocket-Version: 13\r\n\
\r\n";
        client.write_all(request_str.as_bytes()).await.unwrap();

        let req = server.await.unwrap();

        assert!(req.is_websocket);
        assert_eq!(req.path, "/ws");
        assert_eq!(req.method, HttpMethod::GET);
    }

    #[tokio::test]
    async fn test_parse_request_line_directly() {
        let line = "POST /api/v1/resource HTTP/1.1\r\n";
        let parsed = Request::parse_request_line(line).unwrap();
        assert_eq!(parsed.0, "POST");
        assert_eq!(parsed.1, "/api/v1/resource");
        assert_eq!(parsed.2, "HTTP/1.1");
    }

    #[tokio::test]
    async fn test_parse_cookies_raw() {
        let header = "a=1; b=2; c=3";
        let map = Request::parse_cookies_raw(header);
        assert_eq!(map.get("a").unwrap(), "1");
        assert_eq!(map.get("b").unwrap(), "2");
        assert_eq!(map.get("c").unwrap(), "3");
    }

    #[tokio::test]
    async fn test_get_length() {
        let mut headers = HashMap::new();
        headers.insert(HeaderKey::ContentLength, "42".to_string());
        let len = Request::get_length(&headers);
        assert_eq!(len, 42);
    }

    #[tokio::test]
    async fn test_basic_get() {
        let request = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = spawn_request_server(request, "/").await;
        assert_eq!(req.method, HttpMethod::GET);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.headers.get(&HeaderKey::Host).unwrap(), "localhost");
        assert_eq!(req.body.len(), 0);
    }

    #[tokio::test]
    async fn test_post_with_body_and_cookies() {
        let request =
            b"POST /submit HTTP/1.1\r\n\
Host: localhost\r\n\
Content-Length: 3\r\n\
Cookie: a=1; b=2\r\n\
Content-Type: text/plain\r\n\
\r\n\
abc";
        let req = spawn_request_server(request, "/submit").await;
        assert_eq!(req.method, HttpMethod::POST);
        assert_eq!(req.body, b"abc");
        assert_eq!(req.cookies.get("a").unwrap(), "1");
        assert_eq!(req.cookies.get("b").unwrap(), "2");
        assert_eq!(req.content_type.top_level, MediaType::Text);
        assert_eq!(req.content_type.sub_type, "plain");
    }

    #[tokio::test]
    async fn test_multipart_boundary_detection() {
        let boundary = "----boundary123";
        let request =
            format!("POST /upload HTTP/1.1\r\n\
Content-Length: 0\r\n\
Content-Type: multipart/form-data; boundary={}\r\n\r\n", boundary).into_bytes();
        let req = spawn_request_server(&request, "/upload").await;
        assert_eq!(req.multipart_boundary.unwrap(), boundary);
        assert_eq!(req.content_type.top_level, MediaType::Multipart);
        assert_eq!(req.content_type.sub_type, "form-data");
    }

    #[tokio::test]
    async fn test_websocket_detection() {
        let request =
            b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: abc123==\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let req = spawn_request_server(request, "/ws").await;
        assert!(req.is_websocket);
    }

    #[tokio::test]
    async fn test_line_too_long() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, peer) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket.into_split().0);
            let res = Request::read_line_with_limit(&mut reader, TIMEOUT, 5).await;
            assert!(res.is_err());
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"this line is definitely too long\n").await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket.into_split().0);
            let res = Request::read_line_with_limit(&mut reader, TIMEOUT, 1024).await;
            assert!(res.is_err());
        });

        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 直接 drop client，模拟连接关闭
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_invalid_http_method() {
        let request = b"FOO /bar HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, peer) = listener.accept().await.unwrap();
            let reader = BufReader::new(socket.into_split().0);
            let res = Request::new(reader, peer, "/bar").await;
            assert!(res.is_err());
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(request).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_non_utf8_request_line() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, peer) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket.into_split().0);
            let res = Request::read_first_line(&mut reader).await;
            assert!(res.is_err());
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(&[0xff, 0xfe, 0xfd, b'\n']).await.unwrap();
        server.await.unwrap();
    }

    async fn setup_connection(payload: Vec<u8>) -> OwnedReadHalf {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // client
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream.write_all(&payload).await.unwrap();
            // 不 close，保证 peek 可读
        });

        // server
        let (stream, _) = listener.accept().await.unwrap();
        let (read_half, _) = stream.into_split();
        read_half
    }

    #[tokio::test]
    async fn test_is_http_connection_true() {
        let payload = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mut reader = setup_connection(payload.to_vec()).await;

        let result = Request::is_http_connection(&mut reader)
            .await
            .unwrap();

        assert!(result);
    }

    #[tokio::test]
    async fn test_is_http_connection_false() {
        let payload = b"\x16\x03\x01\x02\x00"; // TLS ClientHello 头
        let mut reader = setup_connection(payload.to_vec()).await;

        let result = Request::is_http_connection(&mut reader)
            .await
            .unwrap();

        assert!(!result);
    }

    #[tokio::test]
    async fn test_is_http_connection_empty() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = TcpStream::connect(addr).await.unwrap();
            // 立刻 drop，不写数据
        });

        let (stream, _) = listener.accept().await.unwrap();
        let (mut reader, _) = stream.into_split();

        let result = Request::is_http_connection(&mut reader)
            .await
            .unwrap();

        assert!(!result);
    }
}
