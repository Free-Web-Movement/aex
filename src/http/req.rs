use std::{collections::HashMap, time::Duration};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt},
    time::timeout,
};

use crate::{
    connection::context::{TypeMap, TypeMapExt},
    http::{
        meta::HttpMetadata, middlewares::websocket::WebSocket, params::Params, protocol::{
            content_type::ContentType, header::HeaderKey, media_type::MediaType,
            method::HttpMethod, status::StatusCode, version::HttpVersion,
        }
    },
};
pub const MAX_CAPACITY: i32 = 1024;
pub const TIME_LIMIT: i32 = 500;

pub struct Request<'a, R> {
    pub reader: &'a mut R,
    pub local: &'a mut TypeMap,
}

impl<'a, R> Request<'a, R>
where
    R: AsyncReadExt + AsyncBufReadExt + Unpin,
{
    pub async fn parse_to_local(&mut self) -> anyhow::Result<()> {
        // 1. 解析请求行 (Method, Path, Version)
        let line = self.read_line_with_limit().await?;
        let line_str = std::str::from_utf8(&line).context("Request line not UTF-8")?;
        let mut parts = line_str.split_whitespace();

        let method_str = parts.next().context("Missing method")?;
        let path = parts.next().context("Missing path")?.to_string();
        let version = parts.next().context("Missing version")?.to_string();

        let version = HttpVersion::from_str(&version).context("Unknown HTTP version")?;
        let method = HttpMethod::from_str(method_str).context("Unknown method")?;

        // 2. 解析所有 Headers
        let headers = self.parse_headers_from_reader().await?;

        let server = headers
            .get(&HeaderKey::Server)
            .cloned()
            .unwrap_or_else(|| "".to_string());

        // 3. 提取特定字段 (移植旧逻辑)

        // 3.1 Content-Length
        let length = headers
            .get(&HeaderKey::ContentLength)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);

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
            server,
            length,
            cookies,
            is_websocket: WebSocket::check(method, &headers),
            params: None,
            status: StatusCode::Ok, // 默认状态码为 200
            body: Vec::new(),       // 默认空消息体
            headers,
        };

        self.local.set_value(meta);
        Ok(())
    }

    /// 移植旧的 Cookie 解析逻辑
    fn parse_cookies_raw(&self, header_value: &str) -> HashMap<String, String> {
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

    async fn read_line_with_limit(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(MAX_CAPACITY as usize);
        let n = timeout(
            Duration::from_millis(TIME_LIMIT as u64),
            self.reader.read_until(b'\n', &mut buf),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Read timeout"))??;

        if n == 0 {
            bail!("Connection closed");
        }
        Ok(buf)
    }

    async fn parse_headers_from_reader(&mut self) -> anyhow::Result<HashMap<HeaderKey, String>> {
        let mut map = HashMap::new();
        loop {
            let line_bytes = self.read_line_with_limit().await?;
            let line = std::str::from_utf8(&line_bytes)?.trim_end_matches("\r\n");
            if line.is_empty() {
                break;
            }
            if let Some(pos) = line.find(':') {
                if let Some(key) = HeaderKey::from_str(line[..pos].trim()) {
                    map.insert(key, line[pos + 1..].trim().to_string());
                }
            }
        }
        Ok(map)
    }

    // --- 业务 Getter ---
    pub fn method(&self) -> HttpMethod {
        self.local.get_value::<HttpMetadata>().unwrap().method.clone()
    }

    /// 快速获取所有的 Params
    pub fn params(&self) -> Option<Params> {
        self.local.get_value::<HttpMetadata>().unwrap().params.clone()
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
    /// @param reader: 实现异步读的流（如 TcpStream 的 ReadHalf）
    /// @param local: 用于存储解析结果的 TypeMap
    /// @param peer_addr: 远程节点的物理地址
    pub fn new(reader: &'a mut R, local: &'a mut TypeMap) -> Self {
        Self { reader, local }
    }
}
