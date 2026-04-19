use tokio::io::AsyncWriteExt;

use crate::{
    connection::context::{BoxWriter, LocalTypeMap},
    http::{
        meta::HttpMetadata,
        protocol::{header::HeaderKey, header::Headers, status::StatusCode, version::HttpVersion},
    },
};

fn build_status_line(status: StatusCode, version: HttpVersion) -> Vec<u8> {
    let prefix = match version {
        HttpVersion::Http10 => b"HTTP/1.0 ".to_vec(),
        HttpVersion::Http11 => b"HTTP/1.1 ".to_vec(),
        HttpVersion::Http20 => b"HTTP/2.0 ".to_vec(),
    };
    let status_str = status.to_str();
    let mut buf = prefix;
    buf.extend_from_slice((status as u16).to_string().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(status_str.as_bytes());
    buf
}

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
        
        // Optimized: pre-allocate buffer
        let mut buf = Vec::with_capacity(256);
        
        // Status line
        let status_line = build_status_line(status, version);
        buf.extend_from_slice(&status_line);
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