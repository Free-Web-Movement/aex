#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET = 0,
    HEAD,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH,
    PROPFIND,
    PROPPATCH,
    MKCOL,
    MKCALENDAR,
    COPY,
    MOVE,
    LOCK,
    UNLOCK,
    SEARCH,
    PURGE,
    LINK,
    UNLINK,
}

pub const HTTP_METHODS: [&str; 21] = [
    "GET",
    "HEAD",
    "POST",
    "PUT",
    "DELETE",
    "CONNECT",
    "OPTIONS",
    "TRACE",
    "PATCH",
    "PROPFIND",
    "PROPPATCH",
    "MKCOL",
    "MKCALENDAR",
    "COPY",
    "MOVE",
    "LOCK",
    "UNLOCK",
    "SEARCH",
    "PURGE",
    "LINK",
    "UNLINK",
];

impl HttpMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Some(HttpMethod::GET),
            "HEAD" => Some(HttpMethod::HEAD),
            "POST" => Some(HttpMethod::POST),
            "PUT" => Some(HttpMethod::PUT),
            "DELETE" => Some(HttpMethod::DELETE),
            "CONNECT" => Some(HttpMethod::CONNECT),
            "OPTIONS" => Some(HttpMethod::OPTIONS),
            "TRACE" => Some(HttpMethod::TRACE),
            "PATCH" => Some(HttpMethod::PATCH),
            "PROPFIND" => Some(HttpMethod::PROPFIND),
            "PROPPATCH" => Some(HttpMethod::PROPPATCH),
            "MKCOL" => Some(HttpMethod::MKCOL),
            "MKCALENDAR" => Some(HttpMethod::MKCALENDAR), // <-- 对应新增
            "COPY" => Some(HttpMethod::COPY),
            "MOVE" => Some(HttpMethod::MOVE),
            "LOCK" => Some(HttpMethod::LOCK),
            "UNLOCK" => Some(HttpMethod::UNLOCK),
            "SEARCH" => Some(HttpMethod::SEARCH),
            "PURGE" => Some(HttpMethod::PURGE),
            "LINK" => Some(HttpMethod::LINK),
            "UNLINK" => Some(HttpMethod::UNLINK),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::HEAD => "HEAD",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::CONNECT => "CONNECT",
            HttpMethod::OPTIONS => "OPTIONS",
            HttpMethod::TRACE => "TRACE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::PROPFIND => "PROPFIND",
            HttpMethod::PROPPATCH => "PROPPATCH",
            HttpMethod::MKCOL => "MKCOL",
            HttpMethod::MKCALENDAR => "MKCALENDAR",
            HttpMethod::COPY => "COPY",
            HttpMethod::MOVE => "MOVE",
            HttpMethod::LOCK => "LOCK",
            HttpMethod::UNLOCK => "UNLOCK",
            HttpMethod::SEARCH => "SEARCH",
            HttpMethod::PURGE => "PURGE",
            HttpMethod::LINK => "LINK",
            HttpMethod::UNLINK => "UNLINK",
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_str() {
        let all_methods = [
            HttpMethod::GET,
            HttpMethod::HEAD,
            HttpMethod::POST,
            HttpMethod::PUT,
            HttpMethod::DELETE,
            HttpMethod::CONNECT,
            HttpMethod::OPTIONS,
            HttpMethod::TRACE,
            HttpMethod::PATCH,
            HttpMethod::PROPFIND,
            HttpMethod::PROPPATCH,
            HttpMethod::MKCOL,
            HttpMethod::MKCALENDAR,
            HttpMethod::COPY,
            HttpMethod::MOVE,
            HttpMethod::LOCK,
            HttpMethod::UNLOCK,
            HttpMethod::SEARCH,
            HttpMethod::PURGE,
            HttpMethod::LINK,
            HttpMethod::UNLINK,
        ];

        for method in all_methods.iter() {
            let s = method.to_str();
            assert!(!s.is_empty());
            // 验证 to_str 返回的字符串与常量列表一致
            assert!(HTTP_METHODS.contains(&s));
        }
    }

    #[test]
    fn test_from_str() {
        let all_pairs = [
            ("GET", HttpMethod::GET),
            ("HEAD", HttpMethod::HEAD),
            ("POST", HttpMethod::POST),
            ("PUT", HttpMethod::PUT),
            ("DELETE", HttpMethod::DELETE),
            ("CONNECT", HttpMethod::CONNECT),
            ("OPTIONS", HttpMethod::OPTIONS),
            ("TRACE", HttpMethod::TRACE),
            ("PATCH", HttpMethod::PATCH),
            ("PROPFIND", HttpMethod::PROPFIND),
            ("PROPPATCH", HttpMethod::PROPPATCH),
            ("MKCOL", HttpMethod::MKCOL),
            ("MKCALENDAR", HttpMethod::MKCALENDAR),
            ("COPY", HttpMethod::COPY),
            ("MOVE", HttpMethod::MOVE),
            ("LOCK", HttpMethod::LOCK),
            ("UNLOCK", HttpMethod::UNLOCK),
            ("SEARCH", HttpMethod::SEARCH),
            ("PURGE", HttpMethod::PURGE),
            ("LINK", HttpMethod::LINK),
            ("UNLINK", HttpMethod::UNLINK),
        ];

        for (s, method) in all_pairs.iter() {
            // 精确匹配
            assert_eq!(HttpMethod::from_str(s), Some(*method));
            // 大小写不敏感
            assert_eq!(HttpMethod::from_str(&s.to_ascii_lowercase()), Some(*method));
            assert_eq!(HttpMethod::from_str(&s.to_ascii_uppercase()), Some(*method));
        }

        // 不存在的 method
        assert_eq!(HttpMethod::from_str("FOOBAR"), None);
        assert_eq!(HttpMethod::from_str(""), None);
    }

    #[test]
    fn test_to_str_from_str_roundtrip() {
        for &method_str in HTTP_METHODS.iter() {
            let method = HttpMethod::from_str(method_str).unwrap();
            assert_eq!(method.to_str(), method_str);
        }
    }
}
