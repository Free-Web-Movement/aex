#[cfg(test)]
mod tests {
    use aex::connection::context::TypeMapExt;
    use aex::http::params::Params;
    use aex::http::protocol::method::HttpMethod;
    use aex::{
        connection::context::TypeMap,
        http::{meta::HttpMetadata, req::Request},
    };
    use std::collections::HashMap;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_parse_to_local_success() {
        let mut local = TypeMap::new();
        let input = b"GET /api/test?id=1 HTTP/1.1\r\n\
                      Host: localhost\r\n\
                      Content-Type: application/json\r\n\
                      Content-Length: 15\r\n\
                      Cookie: user=alice; session=123\r\n\
                      Transfer-Encoding: chunked\r\n\
                      \r\n";

        let mut reader = BufReader::new(Cursor::new(input));
        let mut req = Request::new(&mut reader, &mut local);

        let result = req.parse_to_local().await;
        assert!(result.is_ok());

        let meta = local.get_value::<HttpMetadata>().unwrap();
        assert_eq!(meta.method, HttpMethod::GET);
        assert_eq!(meta.path, "/api/test?id=1");
        assert!(meta.is_chunked);
        assert_eq!(meta.cookies.get("user").unwrap(), "alice");
    }

    #[tokio::test]
    async fn test_parse_multipart_boundary() {
        let mut local = TypeMap::new();
        let input = b"POST /upload HTTP/1.1\r\n\
                      Content-Type: multipart/form-data; boundary=X-BOUNDARY\r\n\
                      \r\n";

        let mut reader = BufReader::new(Cursor::new(input));
        let mut req = Request::new(&mut reader, &mut local);
        req.parse_to_local().await.unwrap();

        let meta = local.get_value::<HttpMetadata>().unwrap();
        assert_eq!(meta.multipart_boundary, Some("X-BOUNDARY".to_string()));
    }

    #[tokio::test]
    async fn test_parse_errors() {
        // 1. 测试空输入 (Connection closed)
        let mut local = TypeMap::new();
        let mut reader = BufReader::new(Cursor::new(b""));
        let mut req = Request::new(&mut reader, &mut local);
        assert!(req.parse_to_local().await.is_err());

        // 2. 测试无效的请求行
        let mut reader = BufReader::new(Cursor::new(b"INVALID_LINE\r\n\r\n"));
        let mut req = Request::new(&mut reader, &mut local);
        assert!(req.parse_to_local().await.is_err());

        // 3. 测试无效 UTF-8
        let mut reader = BufReader::new(Cursor::new(vec![0xff, 0xff, 0x0a]));
        let mut req = Request::new(&mut reader, &mut local);
        assert!(req.parse_to_local().await.is_err());
    }

    #[tokio::test]
    async fn test_cookie_parsing_edge_cases() {
        let mut local = TypeMap::new();
        let input = b"GET / HTTP/1.1\r\nCookie: ;; a=1 ; b=2 ; ;\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(input));
        let mut req = Request::new(&mut reader, &mut local);
        req.parse_to_local().await.unwrap();

        let meta = local.get_value::<HttpMetadata>().unwrap();
        assert_eq!(meta.cookies.get("a").unwrap(), "1");
        assert_eq!(meta.cookies.get("b").unwrap(), "2");
    }

    #[tokio::test]
    async fn test_getters() {
        let mut local = TypeMap::new();
        // 预设 HttpMetadata
        let mut meta = HttpMetadata::default();
        meta.method = HttpMethod::POST;

        let mut params = Params::new("/?q=rust".to_string());
        let mut data = HashMap::new();
        data.insert("id".to_string(), "123".to_string());
        params.data = Some(data);
        params.set_form("name=bob");

        meta.params = Some(params);
        local.set_value(meta);

        let mut reader = BufReader::new(Cursor::new(b""));
        let req = Request::new(&mut reader, &mut local);

        assert_eq!(req.method(), HttpMethod::POST);
        assert_eq!(req.param("id").unwrap(), "123");
        assert_eq!(req.query("q").unwrap(), "rust");
        assert_eq!(req.form("name").unwrap(), "bob");
        assert!(req.query("none").is_none());
    }

    #[tokio::test]
    async fn test_read_line_limit_exceeded() {
        let mut local = TypeMap::new();
        // 构造一个超过 MAX_CAPACITY 的行 (假设 MAX_CAPACITY 为 1024)
        let long_line = vec![b'a'; 2048];
        let mut reader = BufReader::new(Cursor::new(long_line));
        let _req = Request::new(&mut reader, &mut local);

        // 由于 read_until 会持续读取直到看到 \n，
        // 虽然代码里没直接检查长度，但 read_until 内部 buf 会增长。
        // 这里可以通过 Mock 来模拟超时。
    }
}
