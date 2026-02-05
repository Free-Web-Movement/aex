use tokio::io::{ AsyncBufReadExt, AsyncReadExt, BufReader };
use tokio::net::tcp::OwnedReadHalf;
use tokio::time::timeout;

use std::net::SocketAddr;
use std::collections::HashMap;
use std::time::Duration;

use anyhow::{ Context, Result, bail };

const HTTP_BUFFER: usize = 8 * 1024;
use crate::params::Params;
use crate::protocol::content_type::ContentType;
use crate::protocol::header::HeaderKey;
use crate::protocol::method::HttpMethod;
use crate::protocol::media_type::MediaType;

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
}

impl Request {
      #[inline]
    pub async fn read_line_with_limit(
        reader: &mut BufReader<OwnedReadHalf>,
        time_limit: Duration,
        capacity_limit: usize
    ) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(128);

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
        })
    }

    // /// 从 BufReader 中 peek 出 HTTP 请求行的 URL（path + query）
    // /// ⚠️ 不消费流，后续 Request::new 可以正常读取
    // pub async fn peek_url(reader: &mut BufReader<OwnedReadHalf>) -> Result<Option<String>> {
    //     let mut buf = [0u8; HTTP_BUFFER]; // peek 缓冲大小，可根据需要调整

    //     // 获取 TcpStream 参考，直接 peek
    //     let stream = reader.get_mut();
    //     let n = stream.peek(&mut buf).await?;

    //     if n == 0 {
    //         return Ok(None); // 连接关闭
    //     }

    //     // 转成 UTF-8
    //     let s = match str::from_utf8(&buf[..n]) {
    //         Ok(s) => s,
    //         Err(_) => {
    //             return Ok(None);
    //         }
    //     };

    //     // HTTP 请求行通常形如 "GET /path?query HTTP/1.1\r\n"
    //     if let Some(end) = s.find("\r\n") {
    //         let request_line = &s[..end];
    //         let parts: Vec<&str> = request_line.split_whitespace().collect();
    //         if parts.len() >= 2 {
    //             let path = parts[1];
    //             return Ok(Some(path.to_string()));
    //         }
    //     }

    //     Ok(None)
    // }

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
}
