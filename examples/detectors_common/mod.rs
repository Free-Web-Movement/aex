//! Shared building blocks for the proxy-protocol detector examples.
//!
//! This module is development material and is intentionally kept out of the
//! aex core: the core detection framework (`aex::unified::detect`) ships only
//! mainstream web protocols (HTTP/1.1, HTTP/2; HTTP/3/QUIC later). Proxy
//! protocols — TLS routing, SOCKS, trojan, VLESS — are composed here on top
//! of the public framework APIs to show how they integrate.
//!
//! Layering:
//!
//! ```text
//! raw bytes ──► sniffing detectors (TlsDetector / SocksDetector / Http*)
//!                    │ first Match wins
//!                    ▼
//!            custom_handler(protocol) dispatch
//!                    │
//!                    ▼
//!     TlsMiddleware::accept (cert/key) ──► decrypted ctx.reader/writer
//!                    ▼
//!     TrojanValidator / VlessValidator ──► business handlers / fallback
//! ```
//!
//! VMess and Shadowsocks are deliberately absent: their headers are
//! encrypted under UUID/PSK-derived keys and are indistinguishable from
//! random bytes without the secret material. Supporting them requires a
//! validator that holds the user database and trial-decrypts — an
//! application concern, not passive detection. "Clash" is a client speaking
//! those protocols, not a wire protocol of its own.

// Each example binary consumes only the subset it needs.
#![allow(dead_code)]

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context as _, Result};
use futures::future::FutureExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadHalf, WriteHalf};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::http::types::Executor;
use aex::unified::detect::{DetectionState, ProtocolDetector, Verdict};

// ---------------------------------------------------------------------------
// Passive detectors (sniffing pipeline)
// ---------------------------------------------------------------------------

/// ClientHello information extracted by [`TlsDetector`], stored in the
/// detection scratch and propagated to handlers through `DetectionState`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TlsClientHello {
    pub sni: Option<String>,
    pub alpn: Vec<String>,
}

/// Passive TLS detector for the sniffing pipeline: matches connections whose
/// first flight is a TLS handshake record and extracts SNI/ALPN when a
/// ClientHello is present.
pub struct TlsDetector;

impl ProtocolDetector for TlsDetector {
    fn name(&self) -> &str {
        "tls"
    }

    fn protocol(&self) -> &str {
        "tls"
    }

    fn max_need(&self) -> Option<usize> {
        Some(4096)
    }

    fn detect(&self, buf: &[u8], state: &mut DetectionState) -> Verdict {
        match parse_client_hello(buf) {
            ClientHelloParse::Complete(hello) => {
                state.set_scratch(hello);
                Verdict::Match
            }
            ClientHelloParse::NeedMore(n) => Verdict::NeedMore(n),
            ClientHelloParse::NotTls => Verdict::Pass,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelloParse {
    Complete(TlsClientHello),
    NeedMore(usize),
    NotTls,
}

/// Parse the leading TLS record of `buf`, expecting a ClientHello.
pub fn parse_client_hello(buf: &[u8]) -> ClientHelloParse {
    if buf.is_empty() || buf[0] != 0x16 {
        return ClientHelloParse::NotTls;
    }
    // legacy_record_version must be 0x03 0x00..=0x04.
    if buf.len() < 3 {
        return ClientHelloParse::NeedMore(3 - buf.len());
    }
    if buf[1] != 0x03 || buf[2] > 0x04 {
        return ClientHelloParse::NotTls;
    }
    if buf.len() < 5 {
        return ClientHelloParse::NeedMore(5 - buf.len());
    }
    let record_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    if record_len == 0 || record_len > (1 << 14) + 2048 {
        return ClientHelloParse::NotTls;
    }
    let total = 5 + record_len;
    if buf.len() < total {
        return ClientHelloParse::NeedMore(total - buf.len());
    }

    let body = &buf[5..total];
    if body.len() < 4 {
        return ClientHelloParse::NeedMore(total - buf.len());
    }
    if body[0] != 0x01 {
        // TLS traffic but not a ClientHello (mid-handshake or resumption):
        // claim it without name information.
        return ClientHelloParse::Complete(TlsClientHello::default());
    }
    let hs_len = u32::from_be_bytes([0, body[1], body[2], body[3]]) as usize;
    let need_hs = (4 + hs_len).saturating_sub(body.len());
    if need_hs > 0 {
        // Handshake may span records; ask for the missing bytes of this one.
        if total + need_hs <= (1 << 14) + 2048 + 5 {
            return ClientHelloParse::NeedMore(need_hs);
        }
        return ClientHelloParse::NotTls;
    }

    let hello_body = &body[4..4 + hs_len];
    complete_or_default(extract_names(hello_body))
}

fn complete_or_default(v: Option<TlsClientHello>) -> ClientHelloParse {
    ClientHelloParse::Complete(v.unwrap_or_default())
}

/// Walk the ClientHello fields up to the extension block and collect SNI +
/// ALPN. Returns `None` on malformed input.
fn extract_names(body: &[u8]) -> Option<TlsClientHello> {
    let mut hello = TlsClientHello::default();
    let mut pos: usize = 2 /* client_version */ + 32 /* random */;

    let sid_len = *body.get(pos)? as usize;
    pos += 1 + sid_len;
    let cs_len = u16::from_be_bytes([*body.get(pos)?, *body.get(pos + 1)?]) as usize;
    pos += 2 + cs_len;
    let comp_len = *body.get(pos)? as usize;
    pos += 1 + comp_len;

    if pos == body.len() {
        return Some(hello);
    }
    if pos + 2 > body.len() {
        return None;
    }
    let ext_total = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
    pos += 2;
    let end = pos.checked_add(ext_total)?;
    if end > body.len() {
        return None;
    }

    while pos + 4 <= end {
        let etype = u16::from_be_bytes([body[pos], body[pos + 1]]);
        let elen = u16::from_be_bytes([body[pos + 2], body[pos + 3]]) as usize;
        pos += 4;
        if pos + elen > end {
            return None;
        }
        let edata = &body[pos..pos + elen];
        match etype {
            0x0000 => hello.sni = parse_sni(edata),
            0x0010 => hello.alpn = parse_alpn(edata),
            _ => {}
        }
        pos += elen;
    }

    Some(hello)
}

fn parse_sni(data: &[u8]) -> Option<String> {
    let list_len = u16::from_be_bytes([*data.first()?, *data.get(1)?]) as usize;
    let end = 2usize.checked_add(list_len)?;
    if end > data.len() {
        return None;
    }
    let mut pos = 2;
    while pos + 3 <= end {
        let name_type = data[pos];
        let name_len = u16::from_be_bytes([data[pos + 1], data[pos + 2]]) as usize;
        pos += 3;
        if pos + name_len > end {
            return None;
        }
        if name_type == 0 {
            return std::str::from_utf8(&data[pos..pos + name_len])
                .ok()
                .map(|s| s.to_string());
        }
        pos += name_len;
    }
    None
}

fn parse_alpn(data: &[u8]) -> Vec<String> {
    if data.len() < 2 {
        return Vec::new();
    }
    let list_len = u16::from_be_bytes([data[0], data[1]]) as usize;
    let end = (2 + list_len).min(data.len());
    let mut protos = Vec::new();
    let mut pos = 2;
    while pos < end {
        let proto_len = data[pos] as usize;
        pos += 1;
        if proto_len == 0 || pos + proto_len > end {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[pos..pos + proto_len]) {
            protos.push(s.to_string());
        }
        pos += proto_len;
    }
    protos
}

/// Which SOCKS dialect matched; stored in the detection scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocksVersion {
    V4,
    V5,
}

/// Passive SOCKS detector: matches SOCKS5 method-selection greetings
/// (`05 N M...`) and SOCKS4/4a connect requests (`04 cmd ...`).
pub struct SocksDetector;

impl ProtocolDetector for SocksDetector {
    fn name(&self) -> &str {
        "socks"
    }

    fn protocol(&self) -> &str {
        "socks"
    }

    fn max_need(&self) -> Option<usize> {
        Some(257)
    }

    fn detect(&self, buf: &[u8], state: &mut DetectionState) -> Verdict {
        if buf.is_empty() {
            return Verdict::Pass;
        }
        match buf[0] {
            0x05 => {
                if buf.len() < 2 {
                    return Verdict::NeedMore(1);
                }
                let n = buf[1] as usize;
                if n == 0 {
                    return Verdict::Pass;
                }
                if buf.len() < 2 + n {
                    return Verdict::NeedMore(2 + n - buf.len());
                }
                state.set_scratch(SocksVersion::V5);
                Verdict::Match
            }
            0x04 => {
                if buf.len() < 2 {
                    return Verdict::NeedMore(1);
                }
                if !matches!(buf[1], 1 | 2) {
                    return Verdict::Pass;
                }
                if buf.len() < 8 {
                    return Verdict::NeedMore(8 - buf.len());
                }
                state.set_scratch(SocksVersion::V4);
                Verdict::Match
            }
            _ => Verdict::Pass,
        }
    }
}

// ---------------------------------------------------------------------------
// IO glue: pair the context's split reader/writer halves into one AsyncRead +
// AsyncWrite so rustls can drive a session over them.
// ---------------------------------------------------------------------------

struct StreamPair {
    reader: BoxReader,
    writer: BoxWriter,
}

impl AsyncRead for StreamPair {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for StreamPair {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

/// Take both stream halves out of a context, leaving it empty.
pub fn take_streams(ctx: &mut Context) -> Result<(BoxReader, BoxWriter)> {
    let reader = ctx
        .reader
        .take()
        .context("context has no reader installed")?;
    let writer = ctx
        .writer
        .take()
        .context("context has no writer installed")?;
    Ok((reader, writer))
}

/// Put stream halves back into a context.
pub fn put_streams(ctx: &mut Context, reader: BoxReader, writer: BoxWriter) {
    ctx.reader = Some(reader);
    ctx.writer = Some(writer);
}

// ---------------------------------------------------------------------------
// TLS termination middleware
// ---------------------------------------------------------------------------

/// Loads a server certificate once and produces TLS-terminating middleware.
///
/// Pass certificate/key paths (or PEM/DER bytes) and every connection run
/// through [`TlsMiddleware::accept`] comes out decrypted in
/// `ctx.reader`/`ctx.writer`.
#[derive(Clone)]
pub struct TlsLoader {
    config: Arc<rustls::ServerConfig>,
}

impl TlsLoader {
    /// Load PEM-encoded certificate chain and private key from disk.
    pub fn from_paths(cert_path: impl AsRef<Path>, key_path: impl AsRef<Path>) -> Result<Self> {
        let cert_pem = std::fs::read(cert_path)?;
        let key_pem = std::fs::read(key_path)?;
        Self::from_pem(&cert_pem, &key_pem)
    }

    /// Build from PEM-encoded bytes.
    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<Self> {
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut &cert_pem[..])
            .collect::<std::result::Result<_, _>>()?;
        if certs.is_empty() {
            bail!("no certificates found in PEM");
        }
        let key = rustls_pemfile::private_key(&mut &key_pem[..])?.ok_or_else(|| anyhow!("no private key found in PEM"))?;
        Self::from_der(certs, key)
    }

    /// Build from already-parsed DER objects. Uses the ring CryptoProvider
    /// explicitly so the binary does not depend on process-level defaults.
    pub fn from_der(certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|e| anyhow!("no usable protocol versions: {e}"))?
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| anyhow!("invalid certificate/key pair: {e}"))?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    /// Advertise ALPN protocol preferences for this loader.
    pub fn with_alpn(self, protocols: &[&str]) -> Self {
        let mut config = (*self.config).clone();
        config.alpn_protocols = protocols.iter().map(|p| p.as_bytes().to_vec()).collect();
        Self {
            config: Arc::new(config),
        }
    }
}

/// Negotiated TLS session details recorded into `ctx.local` after a
/// successful handshake.
#[derive(Debug, Clone, Default)]
pub struct TlsSession {
    pub sni: Option<String>,
    pub alpn: Option<String>,
}

/// Default ceiling for handshake/header validation reads. Malformed peers
/// that stall mid-header are cut here instead of holding the task forever.
pub const VALIDATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub struct TlsMiddleware;

impl TlsMiddleware {
    /// Middleware factory: returns an Executor performing the TLS server
    /// handshake over the context streams. On success the context carries
    /// decrypted streams plus a [`TlsSession`]; on failure it returns false
    /// and the connection should be dropped.
    pub fn accept(loader: TlsLoader) -> Arc<Executor> {
        Arc::new(move |ctx: &mut Context| {
            let loader = loader.clone();
            async move {
                let (reader, writer) = match take_streams(ctx) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("tls middleware: {e}");
                        return false;
                    }
                };

                let acceptor = TlsAcceptor::from(loader.config.clone());
                let pair = StreamPair { reader, writer };
                let handshake = acceptor.accept(pair);
                let tls = match tokio::time::timeout(VALIDATION_TIMEOUT, handshake).await {
                    Ok(Ok(t)) => t,
                    Ok(Err(e)) => {
                        tracing::info!("tls handshake failed: {e}");
                        return false;
                    }
                    Err(_) => {
                        tracing::info!("tls handshake timed out");
                        return false;
                    }
                };

                // Negotiated details are only readable while we own the
                // stream; capture them before splitting into halves.
                let (_, common) = tls.get_ref();
                let session = TlsSession {
                    sni: common.server_name().map(|s| s.to_string()),
                    alpn: common
                        .alpn_protocol()
                        .and_then(|a| String::from_utf8(a.to_vec()).ok()),
                };

                let (read_half, write_half): (ReadHalf<_>, WriteHalf<_>) = tokio::io::split(tls);
                put_streams(
                    ctx,
                    Box::new(tokio::io::BufReader::new(read_half)),
                    Box::new(write_half),
                );
                ctx.local.set_value(session);
                true
            }
            .boxed()
        })
    }
}

// ---------------------------------------------------------------------------
// Trojan / VLESS validators (run after TLS termination)
// ---------------------------------------------------------------------------

/// Downstream target address parsed from a proxy request header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetAddr {
    Ip(std::net::IpAddr),
    Domain(String),
}

impl std::fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetAddr::Ip(ip) => write!(f, "{ip}"),
            TargetAddr::Domain(d) => write!(f, "{d}"),
        }
    }
}

/// Parsed trojan request header (the bytes are consumed from the stream).
#[derive(Debug, Clone)]
pub struct TrojanRequestInfo {
    pub hash: String,
    pub target: TargetAddr,
    pub port: u16,
}

/// Parsed VLESS request header (the bytes are consumed from the stream).
#[derive(Debug, Clone)]
pub struct VlessRequestInfo {
    pub user: [u8; 16],
    pub command: u8,
    pub target: TargetAddr,
    pub port: u16,
}

async fn read_exact_vec(reader: &mut BoxReader, n: usize) -> std::io::Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    reader.read_exact(&mut v).await?;
    Ok(v)
}

fn is_hex(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| b.is_ascii_hexdigit())
}

/// Incrementally read the address following an ATYP byte. Returns the parsed
/// target plus the raw address encoding length.
async fn read_atyp_addr(reader: &mut BoxReader, atyp: u8) -> Option<(TargetAddr, usize)> {
    match atyp {
        0x01 => {
            let raw = read_exact_vec(reader, 4).await.ok()?;
            Some((
                TargetAddr::Ip(std::net::IpAddr::V4(Ipv4Addr::new(raw[0], raw[1], raw[2], raw[3]))),
                4,
            ))
        }
        0x03 => {
            let len_raw = read_exact_vec(reader, 1).await.ok()?;
            let len = len_raw[0] as usize;
            let raw = read_exact_vec(reader, len).await.ok()?;
            let domain = std::str::from_utf8(&raw).ok()?.to_string();
            Some((TargetAddr::Domain(domain), 1 + len))
        }
        0x04 => {
            let raw = read_exact_vec(reader, 16).await.ok()?;
            let octets: [u8; 16] = raw.try_into().unwrap();
            Some((TargetAddr::Ip(std::net::IpAddr::V6(Ipv6Addr::from(octets))), 16))
        }
        _ => None,
    }
}

pub struct TrojanMiddleware;

impl TrojanMiddleware {
    /// Middleware factory validating and stripping a trojan request header
    /// (`sha224-hex CRLF ATYP addr port CRLF`) from the plaintext stream.
    ///
    /// On success returns true; `ctx.local` carries [`TrojanRequestInfo`] and
    /// the stream is positioned at the payload. Malformed headers return
    /// false so aex drops the connection — pair this with a fallback route by
    /// running it *after* routing, not as a filter.
    pub fn validate() -> Arc<Executor> {
        Arc::new(|ctx: &mut Context| {
            async move {
                let (mut reader, writer) = match take_streams(ctx) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("trojan middleware: {e}");
                        return false;
                    }
                };

                // Fixed prefix: hash(56) + CRLF(2) + ATYP(1), then address
                // and port(2)+CRLF(2) — all under one deadline so stalled
                // peers are dropped instead of deadlocking the task.
                let parsed = tokio::time::timeout(VALIDATION_TIMEOUT, async {
                    let head = read_exact_vec(&mut reader, 59).await.ok()?;
                    if &head[56..58] != b"\r\n" || !is_hex(&head[..56]) {
                        return None;
                    }
                    let (target, _addr_len) = read_atyp_addr(&mut reader, head[58]).await?;
                    let tail = read_exact_vec(&mut reader, 4).await.ok()?;
                    if &tail[2..] != b"\r\n" {
                        return None;
                    }
                    let port = u16::from_be_bytes([tail[0], tail[1]]);
                    let hash = String::from_utf8_lossy(&head[..56]).into_owned();
                    Some((hash, target, port))
                })
                .await
                .ok()
                .flatten();
                let Some((hash, target, port)) = parsed else {
                    return false;
                };

                put_streams(ctx, Box::new(reader), writer);
                ctx.local.set_value(TrojanRequestInfo { hash, target, port });
                true
            }
            .boxed()
        })
    }
}

pub struct VlessMiddleware;

impl VlessMiddleware {
    /// Middleware factory validating and stripping a VLESS request header
    /// (`ver=0 uuid16 addonLen addons cmd port atyp addr`).
    ///
    /// This is structural validation only — it proves the plaintext looks
    /// like VLESS but does NOT authenticate the UUID. Real deployments must
    /// compare `ctx.local`'s [`VlessRequestInfo`].user against an access list.
    pub fn validate() -> Arc<Executor> {
        Arc::new(|ctx: &mut Context| {
            async move {
                let (mut reader, writer) = match take_streams(ctx) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("vless middleware: {e}");
                        return false;
                    }
                };

                let parsed = tokio::time::timeout(VALIDATION_TIMEOUT, async {
                    let mut buf = read_exact_vec(&mut reader, 18).await.ok()?;
                    if buf[0] != 0x00 {
                        return None;
                    }
                    let addon_len = buf[17] as usize;
                    buf.extend_from_slice(&read_exact_vec(&mut reader, addon_len + 4).await.ok()?);
                    // Now: [18+addon_len]=cmd, then port(2), atyp(1).
                    let base = 18 + addon_len;
                    if !matches!(buf[base], 1..=3) {
                        return None;
                    }
                    let (target, _addr_len) = read_atyp_addr(&mut reader, buf[base + 3]).await?;
                    let user: [u8; 16] = buf[1..17].try_into().unwrap();
                    Some((
                        user,
                        buf[base],
                        u16::from_be_bytes([buf[base + 1], buf[base + 2]]),
                        target,
                    ))
                })
                .await
                .ok()
                .flatten();
                let Some((user, command, port, target)) = parsed else {
                    return false;
                };

                put_streams(ctx, Box::new(reader), writer);
                ctx.local.set_value(VlessRequestInfo {
                    user,
                    command,
                    port,
                    target,
                });
                true
            }
            .boxed()
        })
    }
}
