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
        let w = self.writer.as_deref_mut().ok_or_else(|| anyhow::anyhow!("Writer not available"))?;
        
        // Optimized: build response in single buffer, write once
        let mut buf = Vec::with_capacity(256);
        
        // Status line: "HTTP/1.1 200 OK\r\n"
        match version {
            HttpVersion::Http10 => buf.extend_from_slice(b"HTTP/1.0 "),
            HttpVersion::Http11 => buf.extend_from_slice(b"HTTP/1.1 "),
            HttpVersion::Http20 => buf.extend_from_slice(b"HTTP/2.0 "),
        }
        // Status code as u16 -> string with text
        buf.extend_from_slice((status as u16).to_string().as_bytes());
        buf.push(b' ');
        buf.extend_from_slice(status.to_str().as_bytes());
        buf.extend_from_slice(b"\r\n");
        
        // Headers
        for (k, v) in headers {
            buf.extend_from_slice(k.as_str().as_bytes());
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(v.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        
        buf.extend_from_slice(b"\r\n");
        buf.extend_from_slice(body);
        
        w.write_all(&buf).await?;
        w.flush().await?;

        Ok(())
    }

    pub fn set_header(&mut self, key: impl Into<HeaderKey>, value: impl Into<String>) -> &mut Self {
        if let Some(meta) = self.local.get_mut::<HttpMetadata>() {
            meta.headers.insert(key.into(), value.into());
        }
        self
    }

    pub async fn send_response(&mut self) -> anyhow::Result<()> {
        let (headers, body, status, version) = {
            let meta = self.local.get_mut::<HttpMetadata>().ok_or_else(|| anyhow::anyhow!("HttpMetadata not found"))?;
            meta.headers.insert(HeaderKey::ContentLength, meta.body.len().to_string());
            (meta.headers.clone(), meta.body.clone(), meta.status, meta.version)
        };
        self.send(&headers, &body, status, version).await
    }

    pub async fn send_failure(&mut self) -> anyhow::Result<()> {
        let (headers, body, status, version) = {
            let meta = self.local.get_mut::<HttpMetadata>().ok_or_else(|| anyhow::anyhow!("HttpMetadata not found"))?;
            if meta.status == StatusCode::Ok {
                meta.status = StatusCode::BadRequest;
            }
            if meta.body.is_empty() {
                meta.body = b"Error".to_vec();
            }
            meta.headers.insert(HeaderKey::ContentLength, meta.body.len().to_string());
            (meta.headers.clone(), meta.body.clone(), meta.status, meta.version)
        };
        self.send(&headers, &body, status, version).await
    }
}