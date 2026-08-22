//! HTTP forward proxy: absolute-form requests and CONNECT tunnels.
//!
//! Triggered purely by the client's request line on an otherwise normal
//! HTTP connection:
//!
//! * `CONNECT host:port HTTP/1.1` — authority-form target; after a `200`
//!   the raw bytes are piped bidirectionally to the upstream.
//! * `GET http://host/path HTTP/1.1` — absolute-form; the request is
//!   rewritten to origin-form and relayed with `Connection: close`.
//!
//! Origin-form requests (`GET /path`) never reach this module — they are
//! website traffic handled by the router/handlers.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::connection::context::{BoxReader, BoxWriter, Context};
use crate::http::meta::HttpMetadata;
use crate::http::protocol::header::HeaderKey;

/// Credential checker for `Proxy-Authorization: Basic ...`. `None` means the
/// proxy is open (logged once at startup by the server builder).
pub type ProxyAuthorizer = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROXY_BODY: usize = 8 * 1024 * 1024;
const VIA: &str = "1.1 aex-proxy";

/// Inspect parsed request metadata and serve proxy traffic. Returns true if
/// the connection was fully handled as proxy traffic (caller must stop
/// routing), false when it is ordinary website traffic.
pub async fn maybe_handle_http_proxy(ctx: &mut Context, authorizer: Option<&ProxyAuthorizer>) -> bool {
    let Some(meta) = ctx.local.get_ref::<HttpMetadata>().cloned() else {
        return false;
    };

    match meta.method {
        crate::http::protocol::method::HttpMethod::CONNECT => {
            handle_connect(ctx, &meta.path, authorizer).await;
            true
        }
        _ if meta.path.starts_with("http://") => {
            handle_absolute_form(ctx, &meta, authorizer).await;
            true
        }
        // https:// absolute-form would require MITM TLS — refuse explicitly.
        _ if meta.path.starts_with("https://") => {
            write_simple_response(ctx, 502, "https absolute-form not supported; use CONNECT\n")
                .await;
            true
        }
        _ => false,
    }
}

async fn handle_connect(ctx: &mut Context, authority: &str, authorizer: Option<&ProxyAuthorizer>) {
    if !authorized(ctx, authorizer) {
        write_simple_response(ctx, 407, "proxy authentication required\n").await;
        return;
    }
    let Some((host, port)) = parse_authority(authority) else {
        write_simple_response(ctx, 400, "CONNECT requires authority-form host:port\n").await;
        return;
    };
    let Ok(upstream) = tokio::time::timeout(UPSTREAM_TIMEOUT, TcpStream::connect((host.as_str(), port))).await
    else {
        write_simple_response(ctx, 504, "upstream connect timed out\n").await;
        return;
    };
    let Ok(upstream) = upstream else {
        write_simple_response(ctx, 502, "cannot reach upstream\n").await;
        return;
    };

    // Only after the tunnel exists do we green-light the client.
    if let Some(w) = ctx.writer.as_mut() {
        let _ = w.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await;
        let _ = w.flush().await;
    }

    let Some((mut client_read, mut client_write)) = take_stream_halves(ctx) else {
        return;
    };
    let (mut up_read, mut up_write) = upstream.into_split();

    let c2u = tokio::io::copy(&mut client_read, &mut up_write);
    let u2c = tokio::io::copy(&mut up_read, &mut client_write);
    let _ = tokio::join!(c2u, u2c);
}

async fn handle_absolute_form(ctx: &mut Context, meta: &HttpMetadata, authorizer: Option<&ProxyAuthorizer>) {
    if !authorized(ctx, authorizer) {
        write_simple_response(ctx, 407, "proxy authentication required\n").await;
        return;
    }
    if meta.is_chunked {
        write_simple_response(ctx, 501, "chunked request bodies are not proxied yet\n").await;
        return;
    }

    let rest = &meta.path["http://".len()..];
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = parse_authority(authority).unwrap_or_else(|| (authority.to_string(), 80));

    // Rebuild as origin-form with hop-by-hop headers stripped.
    let mut head = format!("{} {path} HTTP/1.1\r\nHost: {host}\r\n", method_str(meta));
    for (k, v) in meta.headers.iter() {
        if is_hop_by_hop(k.as_str()) {
            continue;
        }
        head.push_str(k.as_str());
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str(&format!("Connection: close\r\nVia: {VIA}\r\n\r\n"));

    let content_length = meta
        .headers
        .get(&HeaderKey::ContentLength)
        .and_then(|v| v.trim().parse::<usize>().ok());
    let body = match content_length {
        Some(0) | None => Vec::new(),
        Some(n) if n <= MAX_PROXY_BODY => {
            let mut buf = vec![0u8; n];
            let reader = ctx.reader.as_mut();
            let Some(r) = reader else {
                write_simple_response(ctx, 400, "no request stream\n").await;
                return;
            };
            if r.read_exact(&mut buf).await.is_err() {
                write_simple_response(ctx, 400, "truncated request body\n").await;
                return;
            }
            buf
        }
        Some(_) => {
            write_simple_response(ctx, 413, "request body too large\n").await;
            return;
        }
    };

    let upstream = tokio::time::timeout(UPSTREAM_TIMEOUT, TcpStream::connect((host.as_str(), port))).await;
    let Ok(Ok(mut upstream)) = upstream else {
        write_simple_response(ctx, 502, "cannot reach upstream\n").await;
        return;
    };

    if upstream.write_all(head.as_bytes()).await.is_err()
        || (!body.is_empty() && upstream.write_all(&body).await.is_err())
        || upstream.flush().await.is_err()
    {
        write_simple_response(ctx, 502, "write to upstream failed\n").await;
        return;
    }

    // Relay the response verbatim until upstream EOF (Connection: close).
    let Some((_client_read, mut client_write)) = take_stream_halves(ctx) else {
        return;
    };
    let _ = tokio::io::copy(&mut upstream, &mut client_write).await;
    let _ = client_write.shutdown().await;

}

fn authorized(ctx: &Context, authorizer: Option<&ProxyAuthorizer>) -> bool {
    let Some(authz) = authorizer else {
        return true; // open proxy
    };
    let creds = ctx
        .local
        .get_ref::<HttpMetadata>()
        .and_then(|m| {
            m.headers.iter().find(|(k, _)| {
                k.as_str().eq_ignore_ascii_case("proxy-authorization")
            })
        })
        .and_then(|(_, v)| {
            let encoded = v.strip_prefix("Basic ").map(str::trim)?;
            decode_basic_auth(encoded)
        });
    match creds {
        Some((user, pass)) => authz(&user, &pass),
        None => false,
    }
}

fn decode_basic_auth(encoded: &str) -> Option<(String, String)> {
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

fn parse_authority(authority: &str) -> Option<(String, u16)> {
    let authority = authority.rsplit('@').next()?; // strip userinfo if present
    if let Some((host, port)) = authority.rsplit_once(':') {
        let port: u16 = port.parse().ok()?;
        let host = host.trim_matches(['[', ']']).to_string();
        Some((host, port))
    } else {
        Some((authority.to_string(), 80))
    }
}

/// Headers that must not be relayed to the upstream origin.
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-authorization"
            | "proxy-connection"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn method_str(meta: &HttpMetadata) -> &'static str {
    use crate::http::protocol::method::HttpMethod::*;
    match meta.method {
        GET => "GET",
        HEAD => "HEAD",
        POST => "POST",
        PUT => "PUT",
        DELETE => "DELETE",
        CONNECT => "CONNECT",
        OPTIONS => "OPTIONS",
        TRACE => "TRACE",
        PATCH => "PATCH",
        _ => "GET",
    }
}

async fn write_simple_response(ctx: &mut Context, status: u16, body: &str) {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        407 => "Proxy Authentication Required",
        413 => "Payload Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "OK",
    };
    let msg = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Some(w) = ctx.writer.as_mut() {
        let _ = w.write_all(msg.as_bytes()).await;
        let _ = w.shutdown().await;
    }
}

/// Take the boxed stream halves out of a context, typed for piping helpers.
fn take_stream_halves(ctx: &mut Context) -> Option<(BoxReader, BoxWriter)> {
    Some((ctx.reader.take()?, ctx.writer.take()?))
}
