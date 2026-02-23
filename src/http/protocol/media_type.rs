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
        // 只取分号前的部分并修剪空白，处理 "json; charset=utf-8"
        let base_sub = s
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();

        match base_sub.as_str() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_as_str() {
        assert_eq!(MediaType::Text.as_str(), "text");
        assert_eq!(MediaType::Image.as_str(), "image");
        assert_eq!(MediaType::Audio.as_str(), "audio");
        assert_eq!(MediaType::Video.as_str(), "video");
        assert_eq!(MediaType::Application.as_str(), "application");
        assert_eq!(MediaType::Multipart.as_str(), "multipart");
        assert_eq!(MediaType::Message.as_str(), "message");
        assert_eq!(MediaType::Font.as_str(), "font");
        assert_eq!(MediaType::Model.as_str(), "model");
        assert_eq!(MediaType::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_from_str_exact() {
        let all_pairs = [
            ("text", MediaType::Text),
            ("image", MediaType::Image),
            ("audio", MediaType::Audio),
            ("video", MediaType::Video),
            ("application", MediaType::Application),
            ("multipart", MediaType::Multipart),
            ("message", MediaType::Message),
            ("font", MediaType::Font),
            ("model", MediaType::Model),
            ("unknown", MediaType::Unknown),
        ];

        for (s, ty) in all_pairs.iter() {
            assert_eq!(MediaType::from_str(s), *ty);
            // 大小写不敏感
            assert_eq!(MediaType::from_str(&s.to_uppercase()), *ty);
            assert_eq!(MediaType::from_str(&s.to_ascii_lowercase()), *ty);
        }

        // 未知类型
        assert_eq!(MediaType::from_str("foobar"), MediaType::Unknown);
        assert_eq!(MediaType::from_str(""), MediaType::Unknown);
    }

    #[test]
    fn test_fromstr_trait() {
        let ty: MediaType = "text".parse().unwrap();
        assert_eq!(ty, MediaType::Text);

        let ty: MediaType = "IMAGE".parse().unwrap();
        assert_eq!(ty, MediaType::Image);

        let ty: MediaType = "unknown_type".parse().unwrap();
        assert_eq!(ty, MediaType::Unknown);
    }

    #[test]
    fn test_guess() {
        let cases = [
            ("index.html", "text/html"),
            ("style.htm", "text/html"),
            ("main.css", "text/css"),
            ("app.js", "application/javascript"),
            ("data.json", "application/json"),
            ("logo.png", "image/png"),
            ("photo.jpg", "image/jpeg"),
            ("photo.jpeg", "image/jpeg"),
            ("anim.gif", "image/gif"),
            ("readme.txt", "text/plain"),
            ("icon.svg", "image/svg+xml"),
            ("favicon.ico", "image/x-icon"),
            ("file.unknownext", "application/octet-stream"),
            ("noextension", "application/octet-stream"),
        ];

        for (filename, expected) in cases.iter() {
            let path = Path::new(filename);
            assert_eq!(MediaType::guess(path), *expected);
        }
    }
}
