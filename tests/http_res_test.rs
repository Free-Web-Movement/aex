#[cfg(test)]
mod tests {
    use aex::{
        connection::context::{BoxWriter, TypeMap, TypeMapExt},
        http::{
            meta::HttpMetadata,
            protocol::{
                header::{HeaderKey, Headers},
                status::StatusCode,
                version::HttpVersion,
            },
            res::Response,
        },
    };
    use std::{collections::HashMap, sync::Arc};

    #[tokio::test]
    async fn test_send_full_response_manual() {
        use std::io::Cursor;
        use tokio::io::AsyncWriteExt;

        // 1. ⚡ 修改：使用 Cursor 包装 Vec，并让 Box 拥有 Cursor 的所有权
        // 这样就满足了 'static 的要求，因为 Box 现在拥有数据，而不是借用数据
        let mut writer: Option<BoxWriter> =
            Some(Box::new(Cursor::new(Vec::new())));

        let local = Arc::new(TypeMap::new());

        {
            let mut response = Response {
                writer: &mut writer,
                local: local.clone(),
            };

            let mut headers = HashMap::new();
            headers.insert(HeaderKey::ContentType, "text/plain".to_string());
            let body = b"hello world";

            // 2. 执行发送
            response
                .send(&headers, body, StatusCode::Created, HttpVersion::Http11)
                .await
                .expect("Failed to send response");
        }

        // 3. ✅ 核心修改：如何拿回数据？
        // 将 Box 里的 dyn 转回具体的 Cursor<Vec<u8>> 是不行的，
        // 但我们可以把整个 Box 拿出来，通过指针强制转换（在测试中是安全的）
        // 或者最简单的办法：使用 `take()` 拿到 Box，然后利用 Cursor 已经实现了 AsyncWrite 的特性

        let mut boxed_writer = writer.take().unwrap();

        // 为了读取数据，我们需要把这个 Box 里的 Cursor 拿出来
        // 由于我们在测试里，可以使用一种稍显“暴力”但有效的方法：
        // 将 Box<dyn Trait> 转换为 Box<Any>
        // 但如果你不想改生产代码，这里推荐使用一个简单的异步手段提取数据：

        boxed_writer.flush().await.unwrap();

        // ⚡ 终极方案：为了 100% 覆盖且能读取，我们在测试中使用 Cursor，
        // 并通过 unsafe 或者在初始化时就定义好提取路径。
        // 这里提供一个最符合你“直接读取”要求的代码：

        let output_bytes = unsafe {
            let ptr = Box::into_raw(boxed_writer);
            // 这里的 ptr 指向的是 Cursor<Vec<u8>>，我们把它转回来
            let cursor_ptr = ptr as *mut Cursor<Vec<u8>>;
            let data = (*cursor_ptr).get_ref().clone();
            let _ = Box::from_raw(ptr); // 把它转回去防止内存泄漏
            data
        };

        let output_str = std::str::from_utf8(&output_bytes).unwrap();

        // 4. 验证内容
        assert!(output_str.starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(output_str.contains("Content-Type: text/plain\r\n"));
        assert!(output_str.ends_with("\r\n\r\nhello world"));
    }

    #[tokio::test]
    async fn test_send_response_from_metadata() {
        use std::io::Cursor;

        // 1. 准备底层数据
        // 我们需要 Cursor 来拥有 Vec，从而满足 Box 的 'static 要求
        let mut writer_opt: Option<BoxWriter> =
            Some(Box::new(Cursor::new(Vec::new())));
        let local = Arc::new(TypeMap::new());

        // 2. 构造元数据 (保持不变)
        let mut headers_map = HashMap::new();
        headers_map.insert(HeaderKey::Server, "RustServer/1.0".to_string());
        let meta = HttpMetadata {
            // ... 你的字段赋值 ...
            status: StatusCode::NotFound,
            body: b"Not Found :(".to_vec(),
            version: HttpVersion::Http11,
            headers: Headers::from(headers_map),
            ..Default::default()
        };
        local.set_value(meta);

        // 3. 执行发送
        {
            let mut response = Response {
                writer: &mut writer_opt,
                local: local.clone(),
            };
            let result = response.send_response().await;
            assert!(result.is_ok());
        }

        // 4. ⚡ 彻底替换 lock() 的地方：拿回所有权并提取数据
        // 因为 Response 已经释放了借用，我们可以 take 出 Box
        let boxed_writer = writer_opt.take().expect("Writer should exist");

        // 由于无法直接从 dyn AsyncWrite 还原 Cursor，
        // 在不改源码的情况下，测试断言的“终极方案”是利用指针还原数据
        let output_str = unsafe {
            let raw_ptr = Box::into_raw(boxed_writer);
            // 强制转回我们存进去的具体类型 Cursor<Vec<u8>>
            let cursor_ptr = raw_ptr as *mut Cursor<Vec<u8>>;
            let bytes = (*cursor_ptr).get_ref().as_slice();
            let s = std::str::from_utf8(bytes).unwrap().to_string();
            let _ = Box::from_raw(raw_ptr); // 重新装箱防止泄漏
            s
        };

        // 5. 验证
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
    //         local: local.clone(),
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
