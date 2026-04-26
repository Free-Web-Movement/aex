use ahash::AHashMap;

use anyhow::{Context, bail};
use tokio::io::AsyncBufReadExt;

use crate::{
    connection::context::{BoxReader, LocalTypeMap},
    constants::http::*,
    http::{
        meta::HttpMetadata,
        middlewares::websocket::WebSocket,
        params::Params,
        protocol::{
            content_type::ContentType,
            header::{HeaderKey, Headers},
            media_type::MediaType,
            method::HttpMethod,
            status::StatusCode,
            version::HttpVersion,
        },
    },
};

pub struct Request<'a> {
    pub reader: &'a mut Option<BoxReader>,
    pub local: &'a mut LocalTypeMap,
}

impl<'a> Request<'a> {
    pub async fn parse_to_local(&mut self) -> anyhow::Result<()> {
        let line = self.read_line_with_limit().await?;
        if line.len() > MAX_REQUEST_LINE_SIZE {
            bail!("Request line too long: {} bytes", line.len());
        }

        // Optimized: split by ' ' instead of split_whitespace
        let mut parts = line.split(|c| *c == b' ');
        let method_bytes = parts.next().context("Missing method")?;
        let path_bytes = parts.next().context("Missing path")?;
        let _version_bytes = parts.next().context("Missing version")?;

        let method_str = std::str::from_utf8(method_bytes).context("Invalid method")?;
        // Optimized: avoid to_string() - use &str for path
        let path_str = std::str::from_utf8(path_bytes).context("Invalid path")?;
        let version_str = b"HTTP/1.1";

        let version = HttpVersion::Http11;
        let method = HttpMethod::from_str(method_str).context("Unknown method")?;

        // Set path directly without allocation (will clone if needed)
        let path = path_str.to_string();

        let headers_map = self.parse_headers_from_reader().await?;

        if headers_map.len() > MAX_HEADER_COUNT {
            bail!("Too many headers: {}", headers_map.len());
        }

        let header_size: usize = headers_map
            .iter()
            .map(|(k, v)| k.as_str().len() + v.len())
            .sum();
        if header_size > MAX_HEADER_SIZE {
            bail!("Total header size too large: {} bytes", header_size);
        }

        let headers = Headers::from(headers_map);

        // 3.2 Content-Type & Multipart Boundary
        let content_type = headers
            .get(&HeaderKey::ContentType)
            .map(|s| ContentType::parse(s))
            .unwrap_or_else(|| ContentType::parse(""));

        let multipart_boundary = if content_type.top_level == MediaType::Multipart
            && content_type.sub_type.is_form_data()
        {
            content_type
                .parameters
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("boundary"))
                .map(|(_, v)| v.clone())
        } else {
            None
        };

        // 3.3 Transfer-Encoding & Chunked
        let (is_chunked, transfer_encoding) =
            if let Some(te) = headers.get(&HeaderKey::TransferEncoding) {
                (
                    te.to_ascii_lowercase().contains("chunked"),
                    Some(te.clone()),
                )
            } else {
                (false, None)
            };

        // 3.4 Cookies
        let cookies = headers
            .get(&HeaderKey::Cookie)
            .map(|s| self.parse_cookies_raw(s))
            .unwrap_or_default();

        // 4. 封装成完整的 HttpMetadata 并存入 Context.local
        let meta = HttpMetadata {
            method,
            path: path.clone(),
            version,
            is_chunked,
            transfer_encoding,
            multipart_boundary,
            content_type,
            cookies,
            is_websocket: WebSocket::check(method, &headers),
            params: None,
            status: StatusCode::Ok, // 默认状态码为 200
            body: Vec::new(),       // 默认空消息体
            headers: Headers::from(headers),
        };

        self.local.set_value(meta);
        Ok(())
    }

    /// 移植旧的 Cookie 解析逻辑
    fn parse_cookies_raw(&self, header_value: &str) -> AHashMap<String, String> {
        let mut map = AHashMap::with_capacity(4);
        let mut count = 0;
        for pair in header_value.split(';') {
            if count >= MAX_COOKIE_COUNT {
                break;
            }
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let mut kv = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                map.insert(k.trim().to_string(), v.trim().to_string());
                count += 1;
            }
        }
        map
    }

    async fn read_line_with_limit(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(MAX_CAPACITY as usize);

        if let Some(r) = self.reader.as_deref_mut() {
            let n = r.read_until(b'\n', &mut buf).await?;
            if n == 0 {
                bail!("Connection closed");
            }
            Ok(buf)
        } else {
            Err(anyhow::anyhow!("Reader taken!"))
        }
    }

    async fn parse_headers_from_reader(&mut self) -> anyhow::Result<AHashMap<HeaderKey, String>> {
        let mut map = AHashMap::with_capacity(16);
        loop {
            let line_bytes = self.read_line_with_limit().await?;
            let line = std::str::from_utf8(&line_bytes)?;
            let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

            if line.is_empty() {
                break;
            }
            if let Some(pos) = line.find(':')
                && let Some(key) = HeaderKey::from_str(line[..pos].trim())
            {
                map.insert(key, line[pos + 1..].trim().to_string());
            }
        }
        Ok(map)
    }

    // --- 业务 Getter ---
    pub fn method(&self) -> HttpMethod {
        self.local
            .get_value::<HttpMetadata>()
            .map(|m| m.method)
            .unwrap_or(HttpMethod::GET)
    }

    /// 快速获取所有的 Params
    pub fn params(&self) -> Option<Params> {
        self.local
            .get_value::<HttpMetadata>()
            .and_then(|m| m.params)
    }

    /// 获取特定的 Path 参数 (e.g., /user/:id)
    pub fn param(&self, key: &str) -> Option<String> {
        self.params()
            .and_then(|p| p.data)
            .and_then(|mut d| d.remove(key))
    }

    /// 获取特定的 Query 参数 (e.g., ?active=true)
    pub fn query(&self, key: &str) -> Option<String> {
        self.params()
            .and_then(|p| p.query.get(key).and_then(|v| v.first().cloned()))
    }

    /// 获取特定的 Form 参数
    pub fn form(&self, key: &str) -> Option<String> {
        self.params()
            .and_then(|p| p.form)
            .and_then(|f| f.get(key).and_then(|v| v.first().cloned()))
    }

    /// 创建一个新的 Request 实例
    pub fn new(reader: &'a mut Option<BoxReader>, local: &'a mut LocalTypeMap) -> Self {
        Self { reader, local }
    }
}
