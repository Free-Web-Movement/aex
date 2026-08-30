//! SOCKS4/4a and SOCKS5 CONNECT proxy.
//!
//! Runs behind the sniffing pipeline: the [`SocksDetector`] claims the
//! connection from its greeting bytes, then [`socks_tcp_handler`] completes
//! the dialect-specific handshake and relays raw TCP. Only the CONNECT
//! command is implemented — BIND and UDP ASSOCIATE reply "command not
//! supported".

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::connection::context::{BoxReader, BoxWriter, Context};
use crate::unified::TCPHandler;
use crate::unified::detect::{DetectionState, ProtocolDetector, Verdict};

use super::http_proxy::ProxyAuthorizer;

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);

/// Passive SOCKS detector for the sniffing pipeline: recognizes SOCKS5
/// method-selection greetings (`05 N M...`) and SOCKS4/4a connect requests
/// (`04 cmd port ip ...`).
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
                    return Verdict::Pass; // zero methods is invalid
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

/// Which SOCKS dialect matched; stored in the detection scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocksVersion {
    V4,
    V5,
}

#[derive(Debug)]
enum TargetHost {
    V4([u8; 4]),
    V6([u8; 16]),
    Domain(String),
}

#[derive(Debug)]
struct SocksTarget {
    host: TargetHost,
    port: u16,
}

/// Build the TCP handler serving SOCKS on connections claimed as "socks".
pub fn socks_tcp_handler(authorizer: Option<ProxyAuthorizer>) -> TCPHandler {
    Arc::new(move |ctx: Context| {
        let authorizer = authorizer.clone();
        tokio::spawn(async move {
            let mut ctx = ctx;
            if let Err(e) = serve_socks(&mut ctx, authorizer.as_ref()).await {
                tracing::info!("[Socks] session ended: {e}");
            }
        })
    })
}


async fn serve_socks(ctx: &mut Context, authorizer: Option<&ProxyAuthorizer>) -> anyhow::Result<()> {
    let Some(mut reader) = ctx.reader.take() else {
        anyhow::bail!("no reader installed");
    };
    let Some(writer) = ctx.writer.take() else {
        anyhow::bail!("no writer installed");
    };

    // The sniffing phase buffered the greeting bytes into the chained
    // cursor, so reading from the stream replays them first.
    let head = reader.fill_buf().await?;
    let dialect = match head.first() {
        Some(0x04) => SocksVersion::V4,
        Some(0x05) => SocksVersion::V5,
        _ => anyhow::bail!("not a SOCKS greeting"),
    };

    match dialect {
        SocksVersion::V4 => serve_v4(reader, writer).await,
        SocksVersion::V5 => serve_v5(ctx, reader, writer, authorizer).await,
    }
}

async fn serve_v4(mut reader: BoxReader, mut writer: BoxWriter) -> anyhow::Result<()> {
    // VER CMD DSTPORT DSTIP USERID(NTS) [DOMAIN(NTS) for 4a]
    let mut head = vec![0u8; 8];
    reader.read_exact(&mut head).await?;
    let cmd = head[1];
    let port = u16::from_be_bytes([head[2], head[3]]);
    let addr: [u8; 4] = head[4..8].try_into()?;
    let _user = read_cstring(&mut reader).await?;

    if cmd != 0x01 {
        // Only CONNECT; BIND replies "request rejected or failed".
        writer.write_all(&[0x00, 0x5B, 0, 0, 0, 0, 0, 0]).await?;
        anyhow::bail!("v4: only CONNECT supported (cmd={cmd})");
    }

    // SOCKS4a: 0.0.0.x (x != 0) means a domain name follows the userid.
    let target = if addr[..3] == [0, 0, 0] && addr[3] != 0 {
        SocksTarget {
            host: TargetHost::Domain(read_cstring(&mut reader).await?),
            port,
        }
    } else {
        SocksTarget {
            host: TargetHost::V4(addr),
            port,
        }
    };

    connect_and_relay(reader, writer, target, ReplyStyle::ReplyV4).await
}

async fn serve_v5(
    ctx: &mut Context,
    mut reader: BoxReader,
    mut writer: BoxWriter,
    authorizer: Option<&ProxyAuthorizer>,
) -> anyhow::Result<()> {
    // --- method selection ---
    let mut head = [0u8; 2]; // VER NMETHODS
    reader.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    reader.read_exact(&mut methods).await?;

    let chosen = if authorizer.is_some() && methods.contains(&0x02) {
        0x02u8
    } else if authorizer.is_none() && methods.contains(&0x00) {
        0x00
    } else {
        writer.write_all(&[0x05, 0xFF]).await?;
        anyhow::bail!("no acceptable auth method");
    };
    writer.write_all(&[0x05, chosen]).await?;

    // --- RFC 1929 username/password subnegotiation ---
    if chosen == 0x02 {
        let mut ulen = [0u8; 2]; // VER ULEN
        reader.read_exact(&mut ulen).await?;
        let mut user = vec![0u8; ulen[1] as usize];
        reader.read_exact(&mut user).await?;
        let mut plen = [0u8; 1];
        reader.read_exact(&mut plen).await?;
        let mut pass = vec![0u8; plen[0] as usize];
        reader.read_exact(&mut pass).await?;

        let ok = authorizer
            .map(|f| f(&String::from_utf8_lossy(&user), &String::from_utf8_lossy(&pass)))
            .unwrap_or(false);
        writer.write_all(&[0x01, u8::from(!ok)]).await?;
        if !ok {
            anyhow::bail!("auth failed");
        }
        ctx.local
            .set_value(SocksUser(String::from_utf8_lossy(&user).into_owned()));
    }

    // --- request ---
    let mut req = [0u8; 4]; // VER CMD RSV ATYP
    reader.read_exact(&mut req).await?;
    if req[1] != 0x01 {
        reply_v5(&mut writer, 0x07).await?; // command not supported
        anyhow::bail!("only CONNECT supported (cmd={})", req[1]);
    }
    let host = match req[3] {
        0x01 => {
            let mut a = [0u8; 4];
            reader.read_exact(&mut a).await?;
            TargetHost::V4(a)
        }
        0x03 => {
            let mut l = [0u8; 1];
            reader.read_exact(&mut l).await?;
            let mut d = vec![0u8; l[0] as usize];
            reader.read_exact(&mut d).await?;
            TargetHost::Domain(String::from_utf8_lossy(&d).into_owned())
        }
        0x04 => {
            let mut a = [0u8; 16];
            reader.read_exact(&mut a).await?;
            TargetHost::V6(a)
        }
        other => {
            reply_v5(&mut writer, 0x08).await?; // ATYP not supported
            anyhow::bail!("unsupported ATYP {other}");
        }
    };
    let mut p = [0u8; 2];
    reader.read_exact(&mut p).await?;

    connect_and_relay(
        reader,
        writer,
        SocksTarget {
            host,
            port: u16::from_be_bytes(p),
        },
        ReplyStyle::ReplyV5,
    )
    .await
}

/// Failure reporting differs between dialects.
#[allow(dead_code)]
enum ReplyStyle {
    /// SOCKS4: second byte 0x5A=granted / 0x5B=rejected.
    ReplyV4,
    /// SOCKS5: full 10-byte reply with REP code.
    ReplyV5,
}

async fn connect_and_relay(
    mut reader: BoxReader,
    mut writer: BoxWriter,
    target: SocksTarget,
    style: ReplyStyle,
) -> anyhow::Result<()> {
    let upstream = match tokio::time::timeout(UPSTREAM_TIMEOUT, resolve_target(&target)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            report_failure(&mut writer, style, &e).await;
            anyhow::bail!("upstream connect failed: {e}");
        }
        Err(_) => {
            report_failure(&mut writer, style, &std::io::ErrorKind::TimedOut.into()).await;
            anyhow::bail!("upstream connect timed out");
        }
    };

    match style {
        ReplyStyle::ReplyV4 => writer.write_all(&[0x00, 0x5A, 0, 0, 0, 0, 0, 0]).await?,
        ReplyStyle::ReplyV5 => {
            writer
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await?
        }
    }

    // Bidirectional pipe until either side closes.
    let (mut up_read, mut up_write) = upstream.into_split();
    let c2u = tokio::io::copy(&mut reader, &mut up_write);
    let u2c = tokio::io::copy(&mut up_read, &mut writer);
    let _ = tokio::join!(c2u, u2c);
    let _ = writer.shutdown().await;
    Ok(())
}

async fn report_failure(writer: &mut BoxWriter, style: ReplyStyle, _err: &std::io::Error) {
    let frame: &[u8] = match style {
        ReplyStyle::ReplyV4 => &[0x00, 0x5B, 0, 0, 0, 0, 0, 0],
        ReplyStyle::ReplyV5 => &[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0], // host unreachable
    };
    let _ = writer.write_all(frame).await;
}

async fn reply_v5(writer: &mut BoxWriter, code: u8) -> anyhow::Result<()> {
    writer
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}

async fn resolve_target(target: &SocksTarget) -> std::io::Result<TcpStream> {
    let addrs: Vec<SocketAddr> = match &target.host {
        TargetHost::V4(a) => vec![SocketAddr::from((Ipv4Addr::from(*a), target.port))],
        TargetHost::V6(a) => vec![SocketAddr::from((Ipv6Addr::from(*a), target.port))],
        TargetHost::Domain(d) => {
            use std::net::ToSocketAddrs;
            (d.as_str(), target.port)
                .to_socket_addrs()?
                .collect()
        }
    };

    let mut last_err: Option<std::io::Error> = None;
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no address resolved")
    }))
}

async fn read_cstring(reader: &mut BoxReader) -> anyhow::Result<String> {
    let mut out = Vec::with_capacity(32);
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).await?;
        if byte[0] == 0 {
            break;
        }
        out.push(byte[0]);
        if out.len() > 255 {
            anyhow::bail!("identifier too long");
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Authenticated SOCKS5 username recorded after RFC 1929 exchange.
#[derive(Debug, Clone)]
pub struct SocksUser(pub String);
