//! Legacy prefix-based protocol classification.
//!
//! New code should prefer the pluggable [`DetectorRegistry`] pipeline; this
//! helper is kept for compatibility with callers that only need a quick guess
//! from the first bytes.
//!
//! [`DetectorRegistry`]: crate::unified::DetectorRegistry

/// HTTP/2 client connection preface.
pub const H2_CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Request-line prefixes recognised as plaintext HTTP/1.x.
pub const HTTP_METHODS: &[&[u8]] = &[
    b"GET ",
    b"POST ",
    b"PUT ",
    b"DELETE ",
    b"PATCH ",
    b"HEAD ",
    b"OPTIONS ",
    b"CONNECT ",
    b"TRACE ",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    Http11,
    Http2,
    TCP,
    UDP,
    Unknown,
}

impl ProtocolKind {
    /// Classify `bytes` by a leading magic/request-line prefix.
    pub fn detect(bytes: &[u8], is_udp: bool) -> Self {
        if is_udp {
            return ProtocolKind::UDP;
        }

        if bytes.is_empty() {
            return ProtocolKind::Unknown;
        }

        if bytes.starts_with(H2_CONNECTION_PREFACE) {
            return ProtocolKind::Http2;
        }

        for method in HTTP_METHODS {
            if bytes.starts_with(method) {
                return ProtocolKind::Http11;
            }
        }

        ProtocolKind::TCP
    }
}
