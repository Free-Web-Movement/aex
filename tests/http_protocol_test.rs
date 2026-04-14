#[cfg(test)]
mod tests {
    use aex::http::protocol::method::HttpMethod;
    use aex::http::protocol::status::StatusCode;
    use aex::http::protocol::version::HttpVersion;
    use aex::http::protocol::header::HeaderKey;
    use aex::http::protocol::content_type::ContentType;
    use aex::http::protocol::media_type::SubMediaType;

    #[test]
    fn test_method_from_str() {
        assert_eq!(HttpMethod::from_str("GET"), Some(HttpMethod::GET));
        assert_eq!(HttpMethod::from_str("POST"), Some(HttpMethod::POST));
        assert_eq!(HttpMethod::from_str("PUT"), Some(HttpMethod::PUT));
        assert_eq!(HttpMethod::from_str("DELETE"), Some(HttpMethod::DELETE));
        assert_eq!(HttpMethod::from_str("PATCH"), Some(HttpMethod::PATCH));
        assert_eq!(HttpMethod::from_str("OPTIONS"), Some(HttpMethod::OPTIONS));
        assert_eq!(HttpMethod::from_str("HEAD"), Some(HttpMethod::HEAD));
        assert_eq!(HttpMethod::from_str("CONNECT"), Some(HttpMethod::CONNECT));
    }

    #[test]
    fn test_method_to_str() {
        assert_eq!(HttpMethod::GET.to_str(), "GET");
        assert_eq!(HttpMethod::POST.to_str(), "POST");
        assert_eq!(HttpMethod::PUT.to_str(), "PUT");
    }

    #[test]
    fn test_method_is_prefixed() {
        assert!(HttpMethod::is_prefixed("GET / HTTP/1.1"));
        assert!(HttpMethod::is_prefixed("POST /api HTTP/1.1"));
        assert!(!HttpMethod::is_prefixed("INVALID"));
    }

    #[test]
    fn test_status_code_to_http_status() {
        assert_eq!(StatusCode::Ok.to_http_status(), http::StatusCode::OK);
        assert_eq!(StatusCode::NotFound.to_http_status(), http::StatusCode::NOT_FOUND);
        assert_eq!(StatusCode::BadRequest.to_http_status(), http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_status_code_from_u16() {
        assert_eq!(StatusCode::from_u16(200), Some(StatusCode::Ok));
        assert_eq!(StatusCode::from_u16(404), Some(StatusCode::NotFound));
        assert_eq!(StatusCode::from_u16(500), Some(StatusCode::InternalServerError));
    }

    #[test]
    fn test_status_code_to_str() {
        assert_eq!(StatusCode::Ok.to_str(), "OK");
        assert_eq!(StatusCode::NotFound.to_str(), "Not Found");
        assert_eq!(StatusCode::BadRequest.to_str(), "Bad Request");
    }

    #[test]
    fn test_version_as_str() {
        assert_eq!(HttpVersion::Http10.as_str(), "HTTP/1.0");
        assert_eq!(HttpVersion::Http11.as_str(), "HTTP/1.1");
        assert_eq!(HttpVersion::Http20.as_str(), "HTTP/2.0");
    }

    #[test]
    fn test_version_from_str() {
        assert_eq!(HttpVersion::from_str("HTTP/1.0"), Some(HttpVersion::Http10));
        assert_eq!(HttpVersion::from_str("HTTP/1.1"), Some(HttpVersion::Http11));
        assert_eq!(HttpVersion::from_str("HTTP/2.0"), Some(HttpVersion::Http20));
        assert_eq!(HttpVersion::from_str("http/1.1"), Some(HttpVersion::Http11));
    }

    #[test]
    fn test_header_key_standard() {
        assert_eq!(HeaderKey::ContentType.as_str(), "Content-Type");
        assert_eq!(HeaderKey::Host.as_str(), "Host");
        assert_eq!(HeaderKey::Authorization.as_str(), "Authorization");
    }

    #[test]
    fn test_content_type_parsing() {
        let ct = ContentType::parse("application/json");
        assert_eq!(ct.top_level.as_str(), "application");
        assert_eq!(ct.sub_type.as_str(), "json");
    }

    #[test]
    fn test_content_type_to_string() {
        let ct = ContentType::parse("text/html; charset=utf-8");
        assert_eq!(ct.to_string(), "text/html; charset=utf-8");
    }

    #[test]
    fn test_media_type_sub_type() {
        assert_eq!(SubMediaType::Json.as_str(), "json");
        assert_eq!(SubMediaType::Html.as_str(), "html");
    }

    #[tokio::test]
    async fn test_method_is_http_connection() {
        use tokio::io::BufReader;
        use std::io::Cursor;
        
        let data = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let cursor = Cursor::new(data);
        let mut reader = BufReader::new(cursor);
        
        let is_http = HttpMethod::is_http_connection(&mut reader).await.unwrap();
        assert!(is_http);
    }
}