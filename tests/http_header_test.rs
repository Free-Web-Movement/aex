#[cfg(test)]
mod tests {
    use aex::http::protocol::header::HeaderKey;


    // ---------- from_str 标准 header ----------
    #[test]
    fn test_from_str_standard() {
        let key = HeaderKey::from_str("Content-Type").unwrap();
        assert_eq!(key, HeaderKey::ContentType);
    }

    // ---------- 大小写不敏感 ----------
    #[test]
    fn test_from_str_case_insensitive() {
        let key = HeaderKey::from_str("content-type").unwrap();
        assert_eq!(key, HeaderKey::ContentType);

        let key2 = HeaderKey::from_str("CONTENT-TYPE").unwrap();
        assert_eq!(key2, HeaderKey::ContentType);
    }

    // ---------- trim 测试 ----------
    #[test]
    fn test_from_str_trim() {
        let key = HeaderKey::from_str("  Content-Type  ").unwrap();
        assert_eq!(key, HeaderKey::ContentType);
    }

    // ---------- Custom header ----------
    #[test]
    fn test_from_str_custom() {
        let key = HeaderKey::from_str("X-Custom-Header").unwrap();

        match key {
            HeaderKey::Custom(s) => assert_eq!(s, "X-Custom-Header"),
            _ => panic!("Expected Custom variant"),
        }
    }

    // ---------- as_str 标准 header ----------
    #[test]
    fn test_as_str_standard() {
        let key = HeaderKey::ContentLength;
        assert_eq!(key.as_str(), "Content-Length");
    }

    // ---------- as_str custom ----------
    #[test]
    fn test_as_str_custom() {
        let key = HeaderKey::Custom("X-Test".to_string());
        assert_eq!(key.as_str(), "X-Test");
    }

    // ---------- Display ----------
    #[test]
    fn test_display_standard() {
        let key = HeaderKey::ContentType;
        assert_eq!(format!("{}", key), "Content-Type");
    }

    #[test]
    fn test_display_custom() {
        let key = HeaderKey::Custom("X-My-Header".into());
        assert_eq!(format!("{}", key), "X-My-Header");
    }

    // ---------- Hash + Eq 覆盖 ----------
    #[test]
    fn test_eq_and_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(HeaderKey::ContentType);
        set.insert(HeaderKey::ContentType);

        assert_eq!(set.len(), 1);

        set.insert(HeaderKey::Custom("X-A".into()));
        set.insert(HeaderKey::Custom("X-A".into()));

        assert_eq!(set.len(), 2);
    }

    // ---------- 覆盖所有标准 header 至少一次 ----------
    // 这个测试会调用所有 as_str 分支，确保 match 全覆盖
    #[test]
    fn test_all_standard_headers_as_str() {
        let headers = vec![
            HeaderKey::CacheControl,
            HeaderKey::Connection,
            HeaderKey::Date,
            HeaderKey::Pragma,
            HeaderKey::Trailer,
            HeaderKey::TransferEncoding,
            HeaderKey::Upgrade,
            HeaderKey::Via,
            HeaderKey::Warning,
            HeaderKey::Accept,
            HeaderKey::AcceptCharset,
            HeaderKey::AcceptEncoding,
            HeaderKey::AcceptLanguage,
            HeaderKey::Authorization,
            HeaderKey::Cookie,
            HeaderKey::Expect,
            HeaderKey::From,
            HeaderKey::Host,
            HeaderKey::IfMatch,
            HeaderKey::IfModifiedSince,
            HeaderKey::IfNoneMatch,
            HeaderKey::IfRange,
            HeaderKey::IfUnmodifiedSince,
            HeaderKey::MaxForwards,
            HeaderKey::Origin,
            HeaderKey::Range,
            HeaderKey::Referer,
            HeaderKey::TE,
            HeaderKey::UserAgent,
            HeaderKey::AcceptRanges,
            HeaderKey::Age,
            HeaderKey::ETag,
            HeaderKey::Location,
            HeaderKey::ProxyAuthenticate,
            HeaderKey::RetryAfter,
            HeaderKey::Server,
            HeaderKey::SetCookie,
            HeaderKey::Vary,
            HeaderKey::WWWAuthenticate,
            HeaderKey::Allow,
            HeaderKey::ContentEncoding,
            HeaderKey::ContentLanguage,
            HeaderKey::ContentLength,
            HeaderKey::ContentLocation,
            HeaderKey::ContentRange,
            HeaderKey::ContentType,
            HeaderKey::Expires,
            HeaderKey::LastModified,
            HeaderKey::AccessControlAllowCredentials,
            HeaderKey::AccessControlAllowHeaders,
            HeaderKey::AccessControlAllowMethods,
            HeaderKey::AccessControlAllowOrigin,
            HeaderKey::AccessControlExposeHeaders,
            HeaderKey::AccessControlMaxAge,
            HeaderKey::SecFetchDest,
            HeaderKey::SecFetchMode,
            HeaderKey::SecFetchSite,
            HeaderKey::SecFetchUser,
            HeaderKey::SecWebSocketAccept,
            HeaderKey::SecWebSocketExtensions,
            HeaderKey::SecWebSocketKey,
            HeaderKey::SecWebSocketProtocol,
            HeaderKey::SecWebSocketVersion,
            HeaderKey::Forwarded,
            HeaderKey::XForwardedFor,
            HeaderKey::XForwardedHost,
            HeaderKey::XForwardedProto,
            HeaderKey::DNT,
            HeaderKey::KeepAlive,
            HeaderKey::UpgradeInsecureRequests,
        ];

        for header in headers {
            let s = header.as_str();
            assert!(!s.is_empty());
        }
    }
}

