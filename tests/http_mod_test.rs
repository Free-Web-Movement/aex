#[cfg(test)]
mod tests {
    #[test]
    fn test_http_module_re_exports() {
        let _ = aex::http::router::Router::new(aex::http::router::NodeType::Static("root".into()));
        let _ = aex::http::meta::HttpMetadata::default();
    }

    #[test]
    fn test_http_module_protocol_exports() {
        use aex::http::protocol::method::HttpMethod;
        use aex::http::protocol::status::StatusCode;
        use aex::http::protocol::version::HttpVersion;

        assert_eq!(HttpMethod::from_str("GET"), Some(HttpMethod::GET));
        assert_eq!(StatusCode::Ok as u16, 200);
        assert_eq!(HttpVersion::Http11.as_str(), "HTTP/1.1");
    }

    #[test]
    fn test_http_params_empty() {
        let params = aex::http::params::SmallParams::with_capacity(4);
        assert!(params.is_empty());
    }

    #[test]
    fn test_http_router_node_type() {
        use aex::http::router::NodeType;

        let static_node = NodeType::Static("test".to_string());
        let param_node = NodeType::Param("id".to_string());
        let wildcard_node = NodeType::Wildcard;

        assert!(static_node.is_static());
        assert!(param_node.is_param());
        assert!(wildcard_node.is_wildcard());
    }

    #[test]
    fn test_http_metadata_default() {
        let meta = aex::http::meta::HttpMetadata::default();
        assert!(meta.status as u16 > 0);
    }
}
