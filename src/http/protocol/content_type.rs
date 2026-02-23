use crate::http::protocol::media_type::{MediaType, SubMediaType};


/// ContentType 结构
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    pub top_level: MediaType,
    pub sub_type: SubMediaType,
    pub parameters: Vec<(String, String)>,
}

impl ContentType {
pub fn parse(s: &str) -> Self {
        let mut parts = s.split(';');
        let type_part = parts.next().unwrap_or("").trim();

        let mut type_split = type_part.splitn(2, '/');
        let top = type_split.next().unwrap_or("").trim();
        let sub = type_split.next().unwrap_or("").trim();

        ContentType {
            top_level: MediaType::from_str(top),
            sub_type: SubMediaType::from_str(sub),
            parameters: parts
                .map(|p| {
                    let mut kv = p.trim().splitn(2, '=');
                    let k = kv.next().unwrap_or("").trim().to_string();
                    let v = kv.next().unwrap_or("").trim().trim_matches('"').to_string();
                    (k, v)
                })
                .collect(),
        }
    }

    /// 转回字符串
    pub fn to_string(&self) -> String {
        let mut s = format!("{}/{}", self.top_level.as_str(), self.sub_type.as_str());
        for (k, v) in &self.parameters {
            s.push_str(&format!("; {}={}", k, v));
        }
        s
    }

    /// 语义化判断
    pub fn is_form_urlencoded(&self) -> bool {
        self.top_level == MediaType::Application && self.sub_type.is_url_encoded()
    }
}


impl Default for ContentType {
    fn default() -> Self {
        Self {
            top_level: MediaType::Text,
            sub_type: SubMediaType::Plain,
            parameters: Vec::new(),
        }
    }
}

impl ContentType {
    /// 显式创建一个 text/plain 的默认对象
    pub fn new() -> Self {
        Self::default()
    }

    /// 也可以定义一个通用的二进制流默认值
    pub fn octet_stream() -> Self {
        Self {
            top_level: MediaType::Application,
            sub_type: SubMediaType::OctetStream,
            parameters: Vec::new(),
        }
    }
}
