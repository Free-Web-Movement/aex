#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use aex::http::protocol::header::{ HeaderKey, Headers };

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
            HeaderKey::UpgradeInsecureRequests
        ];

        for header in headers {
            let s = header.as_str();
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn test_case_insensitivity() {
        let mut headers = Headers::new();

        // 1. 测试标准 Header 的大小写不敏感
        let key_upper = HeaderKey::from_str("CONTENT-TYPE").unwrap();
        let key_lower = HeaderKey::from_str("content-type").unwrap();

        headers.insert(key_upper.clone(), "application/json");

        // 使用不同大小写的 Key 应该能获取到相同的值
        assert_eq!(headers.get(&key_lower).unwrap(), "application/json");
        assert!(headers.contains(&key_upper));
        assert!(headers.contains(&key_lower));
    }

    #[test]
    fn test_custom_header_case() {
        let mut headers = Headers::new();

        // 2. 测试自定义 Header 的大小写不敏感
        let custom_key1 = HeaderKey::from_str("X-My-Header").unwrap();
        let custom_key2 = HeaderKey::from_str("x-my-header").unwrap();

        headers.insert(custom_key1, "value1");

        // 覆盖更新
        headers.insert(custom_key2.clone(), "value2");

        assert_eq!(headers.len(), 1); // 长度应仍为 1
        assert_eq!(headers.get(&custom_key2).unwrap(), "value2");
    }

    #[test]
    fn test_display_preservation() {
        // 3. 验证虽然 Hash 不区分大小写，但 Display/as_str 保留了原始/定义的格式
        let key = HeaderKey::from_str("uSeR-aGeNt").unwrap();

        // 对于宏里定义的 Standard Header，as_str 始终返回宏定义的字符串
        assert_eq!(key.as_str(), "User-Agent");

        // 对于 Custom Header，保留用户输入的原始格式
        let custom = HeaderKey::from_str("X-tEsT-kEy").unwrap();
        assert_eq!(custom.as_str(), "X-tEsT-kEy");
    }

    #[test]
    fn test_chained_construction() {
        // 4. 测试 with 链式调用
        let headers = Headers::new()
            .with(HeaderKey::Host, "localhost")
            .with(HeaderKey::from_str("X-Request-ID").unwrap(), "12345");

        assert_eq!(headers.get(&HeaderKey::Host).unwrap(), "localhost");
        assert_eq!(headers.get(&HeaderKey::from_str("x-request-id").unwrap()).unwrap(), "12345");
    }

    #[test]
    fn test_deref_and_iteration() {
        // 5. 测试 Deref 带来的 HashMap 特性
        let mut headers = Headers::new();
        headers.insert(HeaderKey::ContentLength, "100");

        // 可以直接调用 HashMap 的方法，如 len()
        assert_eq!(headers.len(), 1);

        // 可以像迭代 HashMap 一样迭代它
        for (k, v) in headers.iter() {
            assert_eq!(k.as_str(), "Content-Length");
            assert_eq!(v, "100");
        }
    }

    #[test]
    fn test_from_iterator() {
        // 6. 测试从数组/迭代器创建
        let data = vec![
            (HeaderKey::Accept, "text/html".to_string()),
            (HeaderKey::from_str("X-Version").unwrap(), "1.0".to_string())
        ];

        let headers: Headers = data.into_iter().collect();
        assert_eq!(headers.len(), 2);
    }

    #[test]
    fn test_remove_case_insensitive() {
        let mut headers = Headers::new();
        headers.insert(HeaderKey::ContentType, "application/json");

        // 1. 使用完全不同的字面量大小写进行删除
        let key_to_remove = HeaderKey::from_str("content-TYPE").unwrap();
        let removed_value = headers.remove(&key_to_remove);

        // 验证：虽然删除时用的是 content-TYPE，但应该能移除 Content-Type
        assert_eq!(removed_value, Some("application/json".to_string()));
        assert_eq!(headers.len(), 0);
        assert!(!headers.contains(&HeaderKey::ContentType));
    }

    #[test]
    fn test_remove_non_existent() {
        let mut headers = Headers::new();
        headers.insert(HeaderKey::Host, "localhost");

        // 尝试删除不存在的 key
        let result = headers.remove(&HeaderKey::from_str("X-Not-Exists").unwrap());
        assert_eq!(result, None);
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn test_from_conversions() {
        // 1. 测试从 HashMap 转换为 Headers
        let mut map = HashMap::new();
        map.insert(HeaderKey::Authorization, "Bearer token123".to_string());

        let headers = Headers::from(map);
        assert!(headers.contains(&HeaderKey::Authorization));
        assert_eq!(headers.get(&HeaderKey::Authorization).unwrap(), "Bearer token123");

        // 2. 测试从 Headers 转回 HashMap
        // 这在需要调用只接受 HashMap 的第三方库时非常有用
        let raw_map: HashMap<HeaderKey, String> = headers.into();
        assert_eq!(raw_map.len(), 1);

        // 验证转回后的 Map 依然保持其 Key 的属性
        let key = HeaderKey::from_str("authorization").unwrap();
        assert!(raw_map.contains_key(&key));
    }
}
