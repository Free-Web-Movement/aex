

#[cfg(test)]
mod tests {
    use aex::http::protocol::header::{HEADER_KEYS, HeaderKey};
    #[test]
    fn test_headerkey_to_str() {
        // 遍历所有枚举，保证 to_str 覆盖
        for i in 0u16..HEADER_KEYS.len() as u16 {
            let key = unsafe { std::mem::transmute::<u16, HeaderKey>(i) };
            let s = key.to_str();
            assert_eq!(s, HEADER_KEYS[i as usize]);
        }
    }

    #[test]
    fn test_headerkey_from_str_exact() {
        // 遍历所有 HEADER_KEYS，确保 from_str 可以匹配
        for i in 0..HEADER_KEYS.len() {
            let key_str = HEADER_KEYS[i];
            let key_enum = HeaderKey::from_str(key_str).unwrap();
            assert_eq!(key_enum.to_str(), key_str);

            // 测试大小写不敏感
            let key_enum_upper = HeaderKey::from_str(&key_str.to_uppercase()).unwrap();
            assert_eq!(key_enum_upper.to_str(), key_str);

            let key_enum_mixed = HeaderKey::from_str(&key_str.to_ascii_lowercase()).unwrap();
            assert_eq!(key_enum_mixed.to_str(), key_str);
        }
    }

    #[test]
    fn test_headerkey_from_str_invalid() {
        // 无效 header 返回 None
        assert!(HeaderKey::from_str("Invalid-Header").is_none());
        assert!(HeaderKey::from_str("").is_none());
        assert!(HeaderKey::from_str(" ").is_none());
        assert!(HeaderKey::from_str("123").is_none());
    }

    #[test]
    fn test_headerkey_all_from_to_roundtrip() {
        // 从枚举 -> str -> 枚举，保证完全一致
        for i in 0u16..HEADER_KEYS.len() as u16 {
            let key = unsafe { std::mem::transmute::<u16, HeaderKey>(i) };
            let s = key.to_str();
            let key_back = HeaderKey::from_str(s).unwrap();
            assert_eq!(key, key_back);
        }
    }
}
