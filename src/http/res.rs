use std::collections::HashMap;
use tokio::io::AsyncWriteExt;

use crate::{
    connection::context::{SharedWriter, TypeMap, TypeMapExt},
    http::{meta::HttpMetadata, protocol::{header::HeaderKey, status::StatusCode, version::HttpVersion}},
};

pub struct Response<'a, W> {
    pub writer: &'a SharedWriter<W>,
    pub local: &'a mut TypeMap,
}

impl<'a, W> Response<'a, W>
where
    W: AsyncWriteExt + Unpin,
{
    pub async fn send_status(
        &self,
        status: StatusCode,
        version: HttpVersion,
    ) -> anyhow::Result<()> {
        let mut w = self.writer.lock().await;
        let res = format!(
            "{} {} {}\r\n\r\n",
            version.as_str(),
            status as u16,
            status.to_str()
        );
        w.write_all(res.as_bytes()).await?;
        w.flush().await?;
        Ok(())
    }

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
            w.write_all(format!("{}: {}\r\n", k.to_str(), v).as_bytes())
                .await?;
        }
        w.write_all(b"\r\n").await?;
        w.write_all(body).await?;
        w.flush().await?;

        Ok(())
    }

    pub async fn send_response(&mut self) -> anyhow::Result<()> {
        println!(
            "Preparing to send response with local context: {:?}",
            self.local.get_value::<HttpMetadata>()
        );

        let meta = self.local.get_value::<HttpMetadata>().unwrap();
        for (k, v) in &meta.headers {
            println!("Header: {}: {}", k.to_str(), v);
        }

        println!("Body: {:?}", String::from_utf8_lossy(&meta.body));

        println!("Status: {}", meta.status.to_str());

        // 2. 调用 send_local。注意这里 headers 传入引用，body 传入引用
        // 如果 send_local 定义需要 &mut HashMap，则传入 &mut headers
        self.send(&meta.headers, &meta.body, meta.status, meta.version)
            .await
    }
}
