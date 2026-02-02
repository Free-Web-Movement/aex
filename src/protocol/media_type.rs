use std::{ path::Path, str::FromStr };

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
}

/// 支持 FromStr trait，方便直接 parse
impl FromStr for MediaType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MediaType::from_str(s))
    }
}
