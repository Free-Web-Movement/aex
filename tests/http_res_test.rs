#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};
    use aex::{connection::context::{TypeMap, TypeMapExt}, http::{meta::HttpMetadata, protocol::{content_type::ContentType, header::{HeaderKey, Headers}, method::HttpMethod, status::StatusCode, version::HttpVersion}, res::Response}};
    use tokio::sync::Mutex;

    // 辅助函数：构造测试用的 Response 环境
    async fn setup_response() -> (Arc<Mutex<Vec<u8>>>, TypeMap) {
        let writer = Arc::new(Mutex::new(Vec::new()));
        let local = TypeMap::new();
        (writer, local)
    }

    #[tokio::test]
    async fn test_send_full_response_manual() {
        let (writer, mut local) = setup_response().await;
        let response = Response {
            writer: &writer,
            local: &mut local,
        };

        let mut headers = HashMap::new();
        headers.insert(HeaderKey::ContentType, "text/plain".to_string());
        let body = b"hello world";

        let result = response.send(
            &headers,
            body,
            StatusCode::Created,
            HttpVersion::Http11
        ).await;

        assert!(result.is_ok());
        let output_bytes = writer.lock().await;
        let output_str = std::str::from_utf8(&output_bytes).unwrap();

        // 验证包含状态行、Header 和 Body
        assert!(output_str.starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(output_str.contains("Content-Type: text/plain\r\n"));
        assert!(output_str.ends_with("\r\n\r\nhello world"));
    }

    #[tokio::test]
    async fn test_send_response_from_metadata() {
        let (writer, mut local) = setup_response().await;
        
        // 构造 HttpMetadata 并存入 local
        let mut headers = HashMap::new();
        headers.insert(HeaderKey::Server, "RustServer/1.0".to_string());
        
        let meta = HttpMetadata {
            method: HttpMethod::GET,
            path: "/".into(),
            version: HttpVersion::Http11,
            is_chunked: false,
            transfer_encoding: None,
            multipart_boundary: None,
            params: None,
            headers: Headers::from(headers),
            content_type: ContentType::default(),
            cookies: HashMap::new(),
            is_websocket: false,
            // server: "RustServer/1.0".into(),
            status: StatusCode::NotFound,
            body: b"Not Found :(".to_vec(),
        };
        local.set_value(meta);

        let mut response = Response {
            writer: &writer,
            local: &mut local,
        };

        let result = response.send_response().await;
        assert!(result.is_ok());

        let output = writer.lock().await;
        let output_str = std::str::from_utf8(&output).unwrap();

        // 验证 Metadata 是否被正确解析并发送
        assert!(output_str.contains("HTTP/1.1 404 Not Found"));
        assert!(output_str.contains("Server: RustServer/1.0"));
        assert!(output_str.contains("Not Found :("));
    }

    // #[tokio::test]
    // async fn test_writer_error_handling() {
    //     // 虽然 Vec<u8> 不会报错，但我们可以验证并发锁是否正常
    //     let (writer, mut local) = setup_response().await;
        
    //     // 先手动持锁，模拟 Writer 忙碌
    //     let _guard = writer.lock().await;
        
    //     let response = Response {
    //         writer: &writer,
    //         local: &mut local,
    //     };

    //     // 尝试发送，这应该会因为获取不到锁而挂起
    //     // 使用 timeout 验证它确实在等待锁
    //     let send_attempt = tokio::time::timeout(
    //         std::time::Duration::from_millis(10),
    //         response.send_status(StatusCode::Ok, HttpVersion::Http11)
    //     ).await;

    //     assert!(send_attempt.is_err(), "应该因为锁被占用而超时");
    // }
}