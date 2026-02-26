use std::collections::HashMap;
use tokio::io::AsyncWriteExt;

use crate::{
    connection::context::{SharedWriter, TypeMap, TypeMapExt},
    http::{
        meta::HttpMetadata,
        protocol::{header::HeaderKey, status::StatusCode, version::HttpVersion},
    },
};

pub struct Response<'a, W> {
    pub writer: &'a SharedWriter<W>,
    pub local: &'a mut TypeMap,
}

impl<'a, W> Response<'a, W>
where
    W: AsyncWriteExt + Unpin,
{
    pub async fn send(
        &self,
        headers: &HashMap<HeaderKey, String>,
        body: &[u8],
        status: StatusCode,
        version: HttpVersion,
    ) -> anyhow::Result<()> {
        // headers.insert("Content-Length".to_string(), body.len().to_string());
        let mut w = self.writer.lock().await;
        w.write_all(format!("{} {} {}\r\n", version, status as u16, status.to_str()).as_bytes())
            .await?;
        for (k, v) in headers {
            w.write_all(format!("{}: {}\r\n", k, v).as_bytes()).await?;
        }
        w.write_all(b"\r\n").await?;
        w.write_all(body).await?;
        w.flush().await?;

        Ok(())
    }

    pub async fn send_response(&mut self) -> anyhow::Result<()> {
        let mut meta = self.local.get_value::<HttpMetadata>().unwrap();
        meta.headers
            .insert(HeaderKey::ContentLength, meta.body.len().to_string());
        self.send(&meta.headers, &meta.body, meta.status, meta.version)
            .await
    }

    pub async fn send_failure(&mut self) -> anyhow::Result<()> {
        let mut meta = self.local.get_value::<HttpMetadata>().unwrap();

        // 如果中间件已经设置了 200 或 Ok，但走到了失败路径，强制修正为 400
        if meta.status == StatusCode::Ok {
            meta.status = StatusCode::BadRequest;
        }

        // 核心改变：如果 body 是空的（说明中间件没写错误详情），我们补一个默认提示
        if meta.body.is_empty() {
            meta.body = format!("Error: {}", meta.status.to_str()).into_bytes();
        }

        // 重新计算并同步 Header
        meta.headers
            .insert(HeaderKey::ContentLength, meta.body.len().to_string());

        // 执行发送
        self.send(&meta.headers, &meta.body, meta.status, meta.version)
            .await
    }
}
