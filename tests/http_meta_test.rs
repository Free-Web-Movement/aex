#[cfg(test)]
mod tests {
    use aex::http::{meta::HttpMetadata, params::Params, protocol::{header::HeaderKey, method::HttpMethod, status::StatusCode}};


    #[test]
    fn test_default_metadata() {
        let meta = HttpMetadata::default();
        
        // 验证基础默认值
        assert_eq!(meta.method, HttpMethod::GET);
        assert_eq!(meta.path, "/");
        assert_eq!(meta.status, StatusCode::Ok);
        assert!(meta.body.is_empty());
        assert!(meta.headers.is_empty());
        assert!(meta.params.is_none());
    }

    #[test]
    fn test_new_metadata() {
        let meta = HttpMetadata::new();
        assert_eq!(meta.path, "/"); // 验证 new 是否调用了 default
    }

    #[test]
    fn test_modification() {
        let mut meta = HttpMetadata::new();
        
        // 修改 path 和 method
        meta.path = "/api/v1/user".to_string();
        meta.method = HttpMethod::POST;
        
        // 模拟 Header 插入
        let key = HeaderKey::from(HeaderKey::ContentType); // 假设 HeaderKey 支持从字符串转换
        meta.headers.insert(key.clone(), "1024".to_string());
        
        assert_eq!(meta.path, "/api/v1/user");
        assert_eq!(meta.method, HttpMethod::POST);
        assert_eq!(meta.headers.get(&key).unwrap(), "1024");
    }

    #[test]
    fn test_params_integration() {
        let mut meta = HttpMetadata::new();
        let url = "/search?q=rust&lang=zh".to_string();
        
        // 模拟 Trie 路由解析后注入 Params
        let params = Params::new(url);
        meta.params = Some(params);
        
        assert!(meta.params.is_some());
        let p = meta.params.as_ref().unwrap();
        assert_eq!(p.query.get("q").unwrap()[0], "rust");
    }

    #[test]
    fn test_body_and_status() {
        let mut meta = HttpMetadata::new();
        
        // 模拟中间件改写状态码和错误消息
        meta.status = StatusCode::NotFound;
        meta.body = "Page Not Found".as_bytes().to_vec();
        
        assert_eq!(meta.status, StatusCode::NotFound);
        assert_eq!(meta.body.len(), 14);
    }
}