use tokio::io::AsyncWriteExt;

use crate::{
    connection::context::{BoxWriter, LocalTypeMap},
    http::{
        meta::HttpMetadata,
        protocol::{header::HeaderKey, header::Headers, status::StatusCode, version::HttpVersion},
    },
};

/// Standard request-only headers that MUST NOT be forwarded into the response.
/// Only headers that are meaningful in a response (Content-Type, Cache-Control,
/// Location, Set-Cookie, etc.) should survive.
const REQUEST_ONLY_HEADERS: [HeaderKey; 25] = [
    // ── General ──
    HeaderKey::Connection,
    HeaderKey::TransferEncoding,
    HeaderKey::Upgrade,
    HeaderKey::Via,
    HeaderKey::Warning,
    // ── Request ──
    HeaderKey::Accept,
    HeaderKey::AcceptCharset,
    HeaderKey::AcceptEncoding,
    HeaderKey::AcceptLanguage,
    HeaderKey::Authorization,
    HeaderKey::Cookie,
    HeaderKey::Expect,
    HeaderKey::From,
    HeaderKey::Host,
    HeaderKey::IfMatch,
    HeaderKey::IfModifiedSince,
    HeaderKey::IfNoneMatch,
    HeaderKey::IfRange,
    HeaderKey::IfUnmodifiedSince,
    HeaderKey::MaxForwards,
    HeaderKey::Origin,
    HeaderKey::Range,
    HeaderKey::Referer,
    HeaderKey::UserAgent,
    // ContentLength is set automatically by build_response, so we strip it too
    HeaderKey::ContentLength,
];

fn status_code_bytes(code: u16) -> [u8; 3] {
    [
        b'0' + (code / 100) as u8,
        b'0' + ((code / 10) % 10) as u8,
        b'0' + (code % 10) as u8,
    ]
}

fn build_status_line(status: StatusCode, version: HttpVersion) -> Vec<u8> {
    let prefix = match version {
        HttpVersion::Http10 => b"HTTP/1.0 ",
        HttpVersion::Http11 => b"HTTP/1.1 ",
        HttpVersion::Http20 => b"HTTP/2.0 ",
    };
    let status_str = status.to_str();
    let mut buf = Vec::with_capacity(prefix.len() + 3 + 1 + status_str.len());
    buf.extend_from_slice(prefix);
    buf.extend_from_slice(&status_code_bytes(status as u16));
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
        let w = self
            .writer
            .as_deref_mut()
            .ok_or_else(|| anyhow::anyhow!("Writer not available"))?;

        let mut buf = Vec::with_capacity(256 + headers.len() * 64);

        let status_line = build_status_line(status, version);
        buf.extend_from_slice(&status_line);
        buf.extend_from_slice(b"\r\n");

        for (k, v) in headers {
            buf.extend_from_slice(k.as_str().as_bytes());
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(v.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }

        buf.extend_from_slice(b"Content-Length: ");
        buf.extend_from_slice(body.len().to_string().as_bytes());
        buf.extend_from_slice(b"\r\n");

        buf.extend_from_slice(b"\r\n");
        buf.extend_from_slice(body);

        crate::connection::metrics::record_global_sent(buf.len() as u64);
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
        let (status, version, body, headers) = {
            let meta = self
                .local
                .get_mut::<HttpMetadata>()
                .ok_or_else(|| anyhow::anyhow!("HttpMetadata not found"))?;
            let body = std::mem::take(&mut meta.body);
            for h in &REQUEST_ONLY_HEADERS {
                meta.headers.remove(h);
            }
            let headers = std::mem::replace(&mut meta.headers, Headers::new());
            (meta.status, meta.version, body, headers)
        };
        self.send(&headers, &body, status, version).await
    }

    pub async fn send_failure(&mut self) -> anyhow::Result<()> {
        let (status, version, body, headers) = {
            let meta = self
                .local
                .get_mut::<HttpMetadata>()
                .ok_or_else(|| anyhow::anyhow!("HttpMetadata not found"))?;
            if meta.status == StatusCode::Ok {
                meta.status = StatusCode::BadRequest;
            }
            if meta.body.is_empty() {
                meta.body = b"Error".to_vec();
            }
            let body = std::mem::take(&mut meta.body);
            for h in &REQUEST_ONLY_HEADERS {
                meta.headers.remove(h);
            }
            let headers = std::mem::replace(&mut meta.headers, Headers::new());
            (meta.status, meta.version, body, headers)
        };
        self.send(&headers, &body, status, version).await
    }
}
