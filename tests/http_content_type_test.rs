
#[cfg(test)]
mod tests {
    use aex::http::protocol::{content_type::ContentType, media_type::{MediaType, SubMediaType}};


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
        assert_eq!(ct.sub_type, SubMediaType::Html);
        assert_eq!(ct.parameters.len(), 1);
        assert_eq!(ct.parameters[0], ("charset".to_string(), "UTF-8".to_string()));

        let ct2 = ContentType::parse("application/json");
        assert_eq!(ct2.top_level, MediaType::Application);
        assert_eq!(ct2.sub_type, SubMediaType::Json);
        assert!(ct2.parameters.is_empty());
    }

    #[test]
    fn test_content_type_to_string() {
        let ct = ContentType::parse("text/html; charset=UTF-8");
        assert_eq!(ct.to_string(), "text/html; charset=UTF-8");
    }

    // --- 1. 测试构造函数与 Default ---
    #[test]
    fn test_constructors() {
        let default_ct = ContentType::default();
        assert_eq!(default_ct.top_level.as_str(), "text");
        assert_eq!(default_ct.sub_type.as_str(), "plain");

        let new_ct = ContentType::new();
        assert_eq!(new_ct, default_ct);

        let octet = ContentType::octet_stream();
        assert_eq!(octet.top_level.as_str(), "application");
        assert_eq!(octet.sub_type.as_str(), "octet-stream");
    }

    // --- 2. 测试解析逻辑 (The Core) ---
    #[test]
    fn test_parse_simple() {
        let s = "application/json";
        let ct = ContentType::parse(s);
        assert_eq!(ct.top_level.as_str(), "application");
        assert_eq!(ct.sub_type.as_str(), "json");
        assert!(ct.parameters.is_empty());
    }

    #[test]
    fn test_parse_with_params() {
        // 测试空格处理、多个参数、带引号的参数
        let s = "text/html; charset=utf-8; boundary=\"something_special\"";
        let ct = ContentType::parse(s);
        
        assert_eq!(ct.top_level.as_str(), "text");
        assert_eq!(ct.sub_type.as_str(), "html");
        assert_eq!(ct.parameters.len(), 2);
        
        assert_eq!(ct.parameters[0], ("charset".to_string(), "utf-8".to_string()));
        // 关键点：trim_matches('"') 必须被覆盖到
        assert_eq!(ct.parameters[1], ("boundary".to_string(), "something_special".to_string()));
    }

    #[test]
    fn test_parse_malformed_and_empty() {
        // 覆盖 type_split.next().unwrap_or("") 的空输入
        let ct = ContentType::parse("");
        assert_eq!(ct.top_level.as_str(), "unknown"); // 假设 from_str("") 返回 Empty/Unknown
        
        // 覆盖只有一个 part 的情况
        let ct = ContentType::parse("application");
        assert_eq!(ct.top_level.as_str(), "application");
        assert_eq!(ct.sub_type.as_str(), "unknown"); // type_split.next() 此时为 None
        
        // 覆盖参数没有 '=' 的情况
        let ct = ContentType::parse("text/plain; invalid_param");
        assert_eq!(ct.parameters[0].0, "invalid_param");
        assert_eq!(ct.parameters[0].1, ""); // kv.next() 为 None
    }

    // --- 3. 测试序列化 (To String) ---
    #[test]
    fn test_to_string() {
        let mut ct = ContentType::octet_stream();
        assert_eq!(ct.to_string(), "application/octet-stream");

        ct.parameters.push(("boundary".to_string(), "abc".to_string()));
        assert_eq!(ct.to_string(), "application/octet-stream; boundary=abc");
    }

    // --- 4. 测试语义化判断 ---
    #[test]
    fn test_semantic_checks() {
        // 路径：匹配成功
        let ct = ContentType::parse("application/x-www-form-urlencoded");
        assert!(ct.is_form_urlencoded());

        // 路径：TopLevel 匹配，SubType 错误
        let ct = ContentType::parse("application/json");
        assert!(!ct.is_form_urlencoded());

        // 路径：TopLevel 错误
        let ct = ContentType::parse("text/x-www-form-urlencoded");
        assert!(!ct.is_form_urlencoded());
    }
}