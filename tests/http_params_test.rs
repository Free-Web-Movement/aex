#[cfg(test)]
mod tests {
    use aex::http::params::Params;
    #[test]
    fn test_new_params_with_query() {
        let url = "https://example.com/search?q=rust&tags=programming&tags=backend".to_string();
        let params = Params::new(url.clone());

        assert_eq!(params.url, url);
        
        // 测试单值参数
        assert_eq!(params.query.get("q").unwrap(), &vec!["rust".to_string()]);
        
        // 测试多值参数 (tags)
        let tags = params.query.get("tags").unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"programming".to_string()));
        assert!(tags.contains(&"backend".to_string()));
    }

    #[test]
    fn test_new_params_without_query() {
        let url = "https://example.com/home".to_string();
        let params = Params::new(url);
        
        assert!(params.query.is_empty());
        assert!(params.data.is_none());
    }

    #[test]
    fn test_parse_special_characters() {
        // 测试 URL 编码字符，如空格 (+) 和特殊符号
        let qs = "name=G%26M&city=New+York";
        let parsed = Params::parse_pairs(qs);

        assert_eq!(parsed.get("name").unwrap()[0], "G&M");
        assert_eq!(parsed.get("city").unwrap()[0], "New York");
    }

    #[test]
    fn test_set_form() {
        let mut params = Params::new("https://api.test".to_string());
        let form_data = "user=alice&token=secret123";
        
        params.set_form(form_data);
        
        let form = params.form.as_ref().expect("Form should be set");
        assert_eq!(form.get("user").unwrap()[0], "alice");
        assert_eq!(form.get("token").unwrap()[0], "secret123");
    }

    #[test]
    fn test_empty_values() {
        let qs = "key1=&key2";
        let parsed = Params::parse_pairs(qs);
        
        // form_urlencoded 规范中，key2 没有值通常会被解析为空字符串
        assert_eq!(parsed.get("key1").unwrap()[0], "");
        assert_eq!(parsed.get("key2").unwrap()[0], "");
    }
}