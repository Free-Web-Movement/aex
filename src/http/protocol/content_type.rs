use crate::http::protocol::media_type::MediaType;


/// ContentType 结构
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    pub top_level: MediaType,
    pub sub_type: String,
    pub parameters: Vec<(String, String)>,
}

impl ContentType {
    /// 从字符串解析，例如：
    /// "text/html; charset=UTF-8"
    pub fn parse(s: &str) -> Self {
        let mut parts = s.split(';');
        let type_part = parts.next().unwrap_or("").trim();

        let mut type_split = type_part.splitn(2, '/');
        let top = type_split.next().unwrap_or("").trim();
        let sub = type_split.next().unwrap_or("").trim();

        let top_level = MediaType::from_str(top);
        let sub_type = sub.to_string(); // 拷贝到 String

        let parameters = parts
            .map(|p| {
                let mut kv = p.trim().splitn(2, '=');
                let k = kv.next().unwrap_or("").trim().to_string();
                let v = kv.next().unwrap_or("").trim().trim_matches('"').to_string();
                (k, v)
            })
            .collect();

        ContentType {
            top_level,
            sub_type,
            parameters,
        }
    }

    /// 转回字符串
    pub fn to_string(&self) -> String {
        let mut s = format!("{}/{}", self.top_level.as_str(), self.sub_type);
        for (k, v) in &self.parameters {
            s.push_str(&format!("; {}={}", k, v));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_top_level_parse() {
        assert_eq!(MediaType::from_str("text"), MediaType::Text);
        assert_eq!(MediaType::from_str("IMAGE"), MediaType::Image);
        assert_eq!(MediaType::from_str("unknown-type"), MediaType::Unknown);
    }

    #[test]
    fn test_content_type_parse() {
        let ct = ContentType::parse("text/html; charset=UTF-8");
        assert_eq!(ct.top_level, MediaType::Text);
        assert_eq!(ct.sub_type, "html");
        assert_eq!(ct.parameters.len(), 1);
        assert_eq!(ct.parameters[0], ("charset".to_string(), "UTF-8".to_string()));

        let ct2 = ContentType::parse("application/json");
        assert_eq!(ct2.top_level, MediaType::Application);
        assert_eq!(ct2.sub_type, "json");
        assert!(ct2.parameters.is_empty());
    }

    #[test]
    fn test_content_type_to_string() {
        let ct = ContentType::parse("text/html; charset=UTF-8");
        assert_eq!(ct.to_string(), "text/html; charset=UTF-8");
    }
}
