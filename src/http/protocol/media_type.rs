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
