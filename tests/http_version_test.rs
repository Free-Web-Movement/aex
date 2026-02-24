#[cfg(test)]
mod tests {
    use aex::http::protocol::version::HttpVersion;

    #[test]
    fn test_http_version_as_str() {
        // 覆盖 as_str 的所有分支
        assert_eq!(HttpVersion::Http10.as_str(), "HTTP/1.0");
        assert_eq!(HttpVersion::Http11.as_str(), "HTTP/1.1");
        assert_eq!(HttpVersion::Http20.as_str(), "HTTP/2.0");
    }

    #[test]
    fn test_http_version_from_str() {
        // 覆盖正常匹配
        assert_eq!(HttpVersion::from_str("HTTP/1.0"), Some(HttpVersion::Http10));
        assert_eq!(HttpVersion::from_str("HTTP/1.1"), Some(HttpVersion::Http11));
        
        // 覆盖 Http2.0 的两个匹配分支
        assert_eq!(HttpVersion::from_str("HTTP/2.0"), Some(HttpVersion::Http20));
        assert_eq!(HttpVersion::from_str("HTTP/2"), Some(HttpVersion::Http20));

        // 覆盖大小写不敏感匹配 (to_ascii_uppercase)
        assert_eq!(HttpVersion::from_str("http/1.1"), Some(HttpVersion::Http11));
        assert_eq!(HttpVersion::from_str("Http/2.0"), Some(HttpVersion::Http20));

        // 覆盖无效输入 (None 分支)
        assert_eq!(HttpVersion::from_str("HTTP/3.0"), None);
        assert_eq!(HttpVersion::from_str("INVALID"), None);
        assert_eq!(HttpVersion::from_str(""), None);
    }

    #[test]
    fn test_http_version_display() {
        // 覆盖 fmt::Display 接口
        assert_eq!(format!("{}", HttpVersion::Http10), "HTTP/1.0");
        assert_eq!(format!("{}", HttpVersion::Http11), "HTTP/1.1");
        assert_eq!(format!("{}", HttpVersion::Http20), "HTTP/2.0");
    }

    #[test]
    fn test_derived_traits() {
        // 覆盖 Clone
        let version = HttpVersion::Http11;
        let cloned = version.clone();
        assert_eq!(version, cloned);

        // 覆盖 Debug
        let debug_str = format!("{:?}", HttpVersion::Http11);
        assert_eq!(debug_str, "Http11");

        // 覆盖 PartialEq/Eq
        assert!(HttpVersion::Http11 == HttpVersion::Http11);
        assert!(HttpVersion::Http11 != HttpVersion::Http10);
    }
}