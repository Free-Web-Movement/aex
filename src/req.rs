use tokio::io::{ AsyncBufReadExt, AsyncReadExt, BufReader };
use tokio::net::tcp::OwnedReadHalf;

use std::net::SocketAddr;
use std::collections::HashMap;

use anyhow::Result;

const HTTP_BUFFER: usize = 8 * 1024;
use crate::params::Params;
use crate::protocol::content_type::ContentType;
use crate::protocol::header::HeaderKey;
use crate::protocol::method::HttpMethod;
use crate::protocol::media_type::MediaType;

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
}

impl Request {
    pub async fn new(
        mut reader: BufReader<OwnedReadHalf>,
        peer_addr: SocketAddr,
        route_pattern: &str // 用于解析动态 path params
    ) -> Self {
        // 1️⃣ 解析请求行
        let mut request_line = String::new();
        reader.read_line(&mut request_line).await.unwrap();
        let parts = request_line.split_whitespace().collect::<Vec<&str>>();
        assert!(parts.len() >= 3, "Invalid HTTP request line");

        let method_str = parts[0];
        let path = parts[1];
        let version = parts[2].to_string();

        // 解析 HttpMethod
        let method = HttpMethod::from_str(method_str).expect(
            &format!("Unsupported HTTP method: {}", method_str)
        );

        println!("Received HTTP request from {}: {} {} {}", peer_addr, method_str, path, version);

        // 2️⃣ 读取 headers
        let headers = Self::read_headers(&mut reader).await;

        // 解析 Cookie
        let cookies = headers
            .get(&HeaderKey::Cookie)
            .map(|s| Self::parse_cookies(s))
            .unwrap_or_default();

        // 判断 Transfer-Encoding
        let (is_chunked, transfer_encoding) = if
            let Some(te) = headers.get(&HeaderKey::TransferEncoding)
        {
            (te.to_ascii_lowercase().contains("chunked"), Some(te.clone()))
        } else {
            (false, None)
        };

        // 3️⃣ 获取 Content-Length
        let length = headers
            .get(&HeaderKey::ContentLength)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);

        // 4️⃣ 读取 body
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).await.unwrap();

        // 5️⃣ 解析 Content-Type
        let content_type = headers
            .get(&HeaderKey::ContentType)
            .map(|s| ContentType::parse(s))
            .unwrap_or_else(|| ContentType::parse(""));

        // 如果是 multipart/form-data，提取 boundary
        let multipart_boundary = if
            content_type.top_level == MediaType::Multipart &&
            content_type.sub_type.to_ascii_lowercase() == "form-data"
        {
            content_type.parameters
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("boundary"))
                .map(|(_, v)| v.clone())
        } else {
            None
        };

        // 6️⃣ 解析动态 path params
        let params = Params::new(path.to_string(), route_pattern.to_string());

        // 7️⃣ 构造 Request
        Request {
            method,
            path: path.to_string(),
            version,
            headers,
            body,
            reader,
            peer_addr,
            params,
            content_type,
            is_chunked,
            transfer_encoding,
            multipart_boundary,
            cookies,
        }
    }

    /// 从 BufReader 中 peek 出 HTTP 请求行的 URL（path + query）
    /// ⚠️ 不消费流，后续 Request::new 可以正常读取
    pub async fn peek_url(reader: &mut BufReader<OwnedReadHalf>) -> Result<Option<String>> {
        let mut buf = [0u8; HTTP_BUFFER]; // peek 缓冲大小，可根据需要调整

        // 获取 TcpStream 参考，直接 peek
        let stream = reader.get_mut();
        let n = stream.peek(&mut buf).await?;

        if n == 0 {
            return Ok(None); // 连接关闭
        }

        // 转成 UTF-8
        let s = match str::from_utf8(&buf[..n]) {
            Ok(s) => s,
            Err(_) => {
                return Ok(None);
            }
        };

        // HTTP 请求行通常形如 "GET /path?query HTTP/1.1\r\n"
        if let Some(end) = s.find("\r\n") {
            let request_line = &s[..end];
            let parts: Vec<&str> = request_line.split_whitespace().collect();
            if parts.len() >= 2 {
                let path = parts[1];
                return Ok(Some(path.to_string()));
            }
        }

        Ok(None)
    }
    /// 将 Cookie header 转换为 HashMap
    fn parse_cookies(header_value: &str) -> HashMap<String, String> {
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
    ) -> HashMap<HeaderKey, String> {
        let mut headers_map: HashMap<HeaderKey, String> = HashMap::new();
        let mut buf = String::new();

        loop {
            buf.clear();
            reader.read_line(&mut buf).await.unwrap();
            if buf == "\r\n" || buf.is_empty() {
                break; // 头结束
            }

            // key: value
            if let Some(pos) = buf.find(":") {
                let key = buf[..pos].trim();
                let value = buf[pos + 1..].trim();

                // 转 HeaderKey 枚举
                if let Some(header_key) = HeaderKey::from_str(key) {
                    headers_map.insert(header_key, value.to_string());
                }
            }
        }

        headers_map
    }
}
