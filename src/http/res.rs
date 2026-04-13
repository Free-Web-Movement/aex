use tokio::io::AsyncWriteExt;

use crate::{
    connection::context::{BoxWriter, LocalTypeMap},
    http::{
        meta::HttpMetadata,
        protocol::{header::HeaderKey, header::Headers, status::StatusCode, version::HttpVersion},
    },
};

pub struct Response<'a> {
    pub writer: &'a mut Option<BoxWriter>,
    pub local: &'a mut LocalTypeMap,
}

impl<'a> Response<'a> {
    pub async fn send(
        &mut self,
        headers: &Headers,
        body: &[u8],
        status: StatusCode,
        version: HttpVersion,
    ) -> anyhow::Result<()> {
        let w = self.writer.as_deref_mut().unwrap();
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

    pub fn set_header(&mut self, key: impl Into<HeaderKey>, value: impl Into<String>) -> &mut Self {
        let meta = self.local.get_mut::<HttpMetadata>().unwrap();
        meta.headers.insert(key.into(), value.into());
        self
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

        if meta.status == StatusCode::Ok {
            meta.status = StatusCode::BadRequest;
        }

        if meta.body.is_empty() {
            meta.body = format!("Error: {}", meta.status.to_str()).into_bytes();
        }

        meta.headers
            .insert(HeaderKey::ContentLength, meta.body.len().to_string());

        self.send(&meta.headers, &meta.body, meta.status, meta.version)
            .await
    }
}

