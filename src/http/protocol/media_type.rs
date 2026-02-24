use std::{path::Path, str::FromStr};

/// Top-level media type 枚举
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MediaType {
    Text = 0,
    Image,
    Audio,
    Video,
    Application,
    Multipart,
    Message,
    Font,
    Model,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubMediaType {
    // Application
    Json,        // application/json
    UrlEncoded,  // application/x-www-form-urlencoded
    OctetStream, // application/octet-stream
    Xml,         // application/xml
    Pdf,         // application/pdf
    Zip,         // application/zip
    Javascript,  // application/javascript

    // Multipart
    FormData, // multipart/form-data
    Mixed,    // multipart/mixed

    // Text
    Plain, // text/plain
    Html,  // text/html
    Css,   // text/css
    Csv,   // text/csv

    // Image
    Png,  // image/png
    Jpeg, // image/jpeg
    Gif,  // image/gif
    Webp, // image/webp
    Svg,  // image/svg+xml
    Icon, // image/x-icon

    // Others
    Wasm, // application/wasm
    Unknown,
}

impl MediaType {
    /// 转换为标准字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaType::Text => "text",
            MediaType::Image => "image",
            MediaType::Audio => "audio",
            MediaType::Video => "video",
            MediaType::Application => "application",
            MediaType::Multipart => "multipart",
            MediaType::Message => "message",
            MediaType::Font => "font",
            MediaType::Model => "model",
            MediaType::Unknown => "unknown",
        }
    }

    /// 从字符串解析 top-level type
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "text" => MediaType::Text,
            "image" => MediaType::Image,
            "audio" => MediaType::Audio,
            "video" => MediaType::Video,
            "application" => MediaType::Application,
            "multipart" => MediaType::Multipart,
            "message" => MediaType::Message,
            "font" => MediaType::Font,
            "model" => MediaType::Model,
            _ => MediaType::Unknown,
        }
    }

    /// 简单 MIME 类型推测
    pub fn guess(path: &Path) -> &'static str {
        match path.extension().and_then(|s| s.to_str()) {
            Some("html") => "text/html",
            Some("htm") => "text/html",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("json") => "application/json",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("txt") => "text/plain",
            Some("svg") => "image/svg+xml",
            Some("ico") => "image/x-icon",
            _ => "application/octet-stream",
        }
    }

    /// 通用匹配判断
    pub fn is_type(&self, other: MediaType) -> bool {
        *self == other
    }

    /// 快捷判断：是否为 Application 类型
    pub fn is_application(&self) -> bool {
        matches!(self, MediaType::Application)
    }

    /// 快捷判断：是否为文本类型
    pub fn is_text(&self) -> bool {
        matches!(self, MediaType::Text)
    }

    /// 快捷判断：是否为多部分表单类型
    pub fn is_multipart(&self) -> bool {
        matches!(self, MediaType::Multipart)
    }
}

/// 支持 FromStr trait，方便直接 parse
impl FromStr for MediaType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MediaType::from_str(s))
    }
}

impl SubMediaType {
    /// 转换为标准 MIME 字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            SubMediaType::Json => "json",
            SubMediaType::UrlEncoded => "x-www-form-urlencoded",
            SubMediaType::OctetStream => "octet-stream",
            SubMediaType::Xml => "xml",
            SubMediaType::Pdf => "pdf",
            SubMediaType::Zip => "zip",
            SubMediaType::Javascript => "javascript",
            SubMediaType::FormData => "form-data",
            SubMediaType::Mixed => "mixed",
            SubMediaType::Plain => "plain",
            SubMediaType::Html => "html",
            SubMediaType::Css => "css",
            SubMediaType::Csv => "csv",
            SubMediaType::Png => "png",
            SubMediaType::Jpeg => "jpeg",
            SubMediaType::Gif => "gif",
            SubMediaType::Webp => "webp",
            SubMediaType::Svg => "svg+xml",
            SubMediaType::Icon => "x-icon",
            SubMediaType::Wasm => "wasm",
            SubMediaType::Unknown => "unknown",
        }
    }

    /// 从 Content-Type 的子类型部分解析
    pub fn from_str(s: &str) -> Self {
        // 1. 先按分号分割，取第一部分
        let type_part = s.split(';').next().unwrap_or("").trim();

        // 2. 如果包含斜杠 (例如 "text/plain")，只取斜杠后面的部分
        let base_sub = if let Some(pos) = type_part.find('/') {
            &type_part[pos + 1..]
        } else {
            type_part
        };

        // 3. 转换为小写并匹配
        match base_sub.to_ascii_lowercase().as_str() {
            "json" => SubMediaType::Json,
            "x-www-form-urlencoded" => SubMediaType::UrlEncoded,
            "form-data" => SubMediaType::FormData,
            "octet-stream" => SubMediaType::OctetStream,
            "xml" => SubMediaType::Xml,
            "html" => SubMediaType::Html,
            "plain" => SubMediaType::Plain,
            "css" => SubMediaType::Css,
            "javascript" | "x-javascript" => SubMediaType::Javascript,
            "png" => SubMediaType::Png,
            "jpeg" | "jpg" => SubMediaType::Jpeg,
            "gif" => SubMediaType::Gif,
            "webp" => SubMediaType::Webp,
            "svg+xml" => SubMediaType::Svg,
            "x-icon" => SubMediaType::Icon,
            "pdf" => SubMediaType::Pdf,
            "zip" => SubMediaType::Zip,
            "wasm" => SubMediaType::Wasm,
            "mixed" => SubMediaType::Mixed,
            "csv" => SubMediaType::Csv,
            _ => SubMediaType::Unknown,
        }
    }

    /// 自动映射到对应的 Top-Level MediaType
    pub fn top_level(&self) -> MediaType {
        match self {
            SubMediaType::Json
            | SubMediaType::UrlEncoded
            | SubMediaType::OctetStream
            | SubMediaType::Xml
            | SubMediaType::Pdf
            | SubMediaType::Zip
            | SubMediaType::Javascript
            | SubMediaType::Wasm => MediaType::Application,

            SubMediaType::FormData | SubMediaType::Mixed => MediaType::Multipart,

            SubMediaType::Plain | SubMediaType::Html | SubMediaType::Css | SubMediaType::Csv => {
                MediaType::Text
            }

            SubMediaType::Png
            | SubMediaType::Jpeg
            | SubMediaType::Gif
            | SubMediaType::Webp
            | SubMediaType::Svg
            | SubMediaType::Icon => MediaType::Image,

            SubMediaType::Unknown => MediaType::Unknown,
        }
    }

    /// 通用匹配判断
    pub fn is_type(&self, other: SubMediaType) -> bool {
        *self == other
    }

    /// 判断是否为表单提交 (URL Encoded)
    pub fn is_url_encoded(&self) -> bool {
        matches!(self, SubMediaType::UrlEncoded)
    }

    /// 判断是否为文件上传 (Multipart)
    pub fn is_form_data(&self) -> bool {
        matches!(self, SubMediaType::FormData)
    }

    /// 判断是否为 JSON
    pub fn is_json(&self) -> bool {
        matches!(self, SubMediaType::Json)
    }

    /// 判断是否为静态 Web 资源 (HTML/CSS/JS)
    pub fn is_web_resource(&self) -> bool {
        matches!(
            self,
            SubMediaType::Html | SubMediaType::Css | SubMediaType::Javascript
        )
    }

    /// 判断是否为图片类型
    pub fn is_image(&self) -> bool {
        matches!(
            self,
            SubMediaType::Png
                | SubMediaType::Jpeg
                | SubMediaType::Gif
                | SubMediaType::Webp
                | SubMediaType::Svg
        )
    }
}

/// 支持 FromStr trait，方便直接 parse
impl FromStr for SubMediaType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(SubMediaType::from_str(s))
    }
}
