use std::collections::HashMap;

use crate::http::{params::Params, protocol::{content_type::ContentType, header::HeaderKey, method::HttpMethod, status::StatusCode, version::HttpVersion}};

// 常规的HTTP请求元数据，供中间件和处理器使用
#[derive(Debug, Clone)]
pub struct HttpMetadata {
    pub method: HttpMethod,
    pub path: String,
    pub version: HttpVersion,
    pub is_chunked: bool,
    pub transfer_encoding: Option<String>,
    pub multipart_boundary: Option<String>,
    pub params: Option<Params>, // 放在Trie路由里解析
    pub headers: HashMap<HeaderKey, String>,
    pub content_type: ContentType,
    pub length: usize,
    pub cookies: HashMap<String, String>,
    pub is_websocket: bool,
    pub server: String,
    //
    pub status: StatusCode, // 处理结果状态码，默认200

    // 如果是form-url-encoded的请求，form会被保存在Params里面
    // body的具体实现不同，请求需要不同的body处理方式（如chunked、websocket等），
    // 所以不直接放在HttpMetadata里，而是根据需要在中间件里动态解析和存储
    pub body: Vec<u8>, // 处理结果消息体（如验证错误信息等），默认空
}


impl Default for HttpMetadata {
    fn default() -> Self {
        Self {
            method: HttpMethod::GET, // 默认 GET
            path: "/".to_string(),
            version: HttpVersion::Http11,
            is_chunked: false,
            transfer_encoding: None,
            multipart_boundary: None,
            params: None,
            headers: HashMap::new(),
            // 假设 ContentType 有默认值（通常是 text/plain 或 application/octet-stream）
            content_type: ContentType::default(),
            length: 0,
            cookies: HashMap::new(),
            is_websocket: false,
            server: "".to_string(),
            status: StatusCode::Ok, // 默认 200 OK
            body: Vec::new(),
        }
    }
}

impl HttpMetadata {
    /// 创建一个基础的元数据对象
    pub fn new() -> Self {
        Self::default()
    }
}
