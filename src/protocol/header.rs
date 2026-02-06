#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum HeaderKey {
    // ===== General Headers =====
    CacheControl = 0,
    Connection,
    Date,
    Pragma,
    Trailer,
    TransferEncoding,
    Upgrade,
    Via,
    Warning,

    // ===== Request Headers =====
    Accept,
    AcceptCharset,
    AcceptEncoding,
    AcceptLanguage,
    Authorization,
    Cookie,
    Expect,
    From,
    Host,
    IfMatch,
    IfModifiedSince,
    IfNoneMatch,
    IfRange,
    IfUnmodifiedSince,
    MaxForwards,
    Origin,
    Range,
    Referer,
    TE,
    UserAgent,

    // ===== Response Headers =====
    AcceptRanges,
    Age,
    ETag,
    Location,
    ProxyAuthenticate,
    RetryAfter,
    Server,
    SetCookie,
    Vary,
    WWWAuthenticate,

    // ===== Entity / Representation Headers =====
    Allow,
    ContentEncoding,
    ContentLanguage,
    ContentLength,
    ContentLocation,
    ContentRange,
    ContentType,
    Expires,
    LastModified,

    // ===== CORS / Fetch / Web =====
    AccessControlAllowCredentials,
    AccessControlAllowHeaders,
    AccessControlAllowMethods,
    AccessControlAllowOrigin,
    AccessControlExposeHeaders,
    AccessControlMaxAge,

    SecFetchDest,
    SecFetchMode,
    SecFetchSite,
    SecFetchUser,

    // ===== WebSocket =====
    SecWebSocketAccept,
    SecWebSocketExtensions,
    SecWebSocketKey,
    SecWebSocketProtocol,
    SecWebSocketVersion,

    // ===== Proxy / Forwarded =====
    Forwarded,
    XForwardedFor,
    XForwardedHost,
    XForwardedProto,

    // ===== Misc / De-facto standard =====
    DNT,
    KeepAlive,
    UpgradeInsecureRequests,
}

pub const HEADER_KEYS: [&str; 70] = [
    // ===== General =====
    "Cache-Control",
    "Connection",
    "Date",
    "Pragma",
    "Trailer",
    "Transfer-Encoding",
    "Upgrade",
    "Via",
    "Warning",

    // ===== Request =====
    "Accept",
    "Accept-Charset",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cookie",
    "Expect",
    "From",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "If-Range",
    "If-Unmodified-Since",
    "Max-Forwards",
    "Origin",
    "Range",
    "Referer",
    "TE",
    "User-Agent",

    // ===== Response =====
    "Accept-Ranges",
    "Age",
    "ETag",
    "Location",
    "Proxy-Authenticate",
    "Retry-After",
    "Server",
    "Set-Cookie",
    "Vary",
    "WWW-Authenticate",

    // ===== Entity =====
    "Allow",
    "Content-Encoding",
    "Content-Language",
    "Content-Length",
    "Content-Location",
    "Content-Range",
    "Content-Type",
    "Expires",
    "Last-Modified",

    // ===== CORS / Fetch =====
    "Access-Control-Allow-Credentials",
    "Access-Control-Allow-Headers",
    "Access-Control-Allow-Methods",
    "Access-Control-Allow-Origin",
    "Access-Control-Expose-Headers",
    "Access-Control-Max-Age",

    "Sec-Fetch-Dest",
    "Sec-Fetch-Mode",
    "Sec-Fetch-Site",
    "Sec-Fetch-User",

    // ===== WebSocket =====
    "Sec-WebSocket-Accept",
    "Sec-WebSocket-Extensions",
    "Sec-WebSocket-Key",
    "Sec-WebSocket-Protocol",
    "Sec-WebSocket-Version",

    // ===== Proxy =====
    "Forwarded",
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",

    // ===== Misc =====
    "DNT",
    "Keep-Alive",
    "Upgrade-Insecure-Requests",
];

impl HeaderKey {
    /// 大小写不敏感匹配字符串到枚举
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            // ===== General =====
            "cache-control" => Some(HeaderKey::CacheControl),
            "connection" => Some(HeaderKey::Connection),
            "date" => Some(HeaderKey::Date),
            "pragma" => Some(HeaderKey::Pragma),
            "trailer" => Some(HeaderKey::Trailer),
            "transfer-encoding" => Some(HeaderKey::TransferEncoding),
            "upgrade" => Some(HeaderKey::Upgrade),
            "via" => Some(HeaderKey::Via),
            "warning" => Some(HeaderKey::Warning),

            // ===== Request =====
            "accept" => Some(HeaderKey::Accept),
            "accept-charset" => Some(HeaderKey::AcceptCharset),
            "accept-encoding" => Some(HeaderKey::AcceptEncoding),
            "accept-language" => Some(HeaderKey::AcceptLanguage),
            "authorization" => Some(HeaderKey::Authorization),
            "cookie" => Some(HeaderKey::Cookie),
            "expect" => Some(HeaderKey::Expect),
            "from" => Some(HeaderKey::From),
            "host" => Some(HeaderKey::Host),
            "if-match" => Some(HeaderKey::IfMatch),
            "if-modified-since" => Some(HeaderKey::IfModifiedSince),
            "if-none-match" => Some(HeaderKey::IfNoneMatch),
            "if-range" => Some(HeaderKey::IfRange),
            "if-unmodified-since" => Some(HeaderKey::IfUnmodifiedSince),
            "max-forwards" => Some(HeaderKey::MaxForwards),
            "origin" => Some(HeaderKey::Origin),
            "range" => Some(HeaderKey::Range),
            "referer" => Some(HeaderKey::Referer),
            "te" => Some(HeaderKey::TE),
            "user-agent" => Some(HeaderKey::UserAgent),

            // ===== Response =====
            "accept-ranges" => Some(HeaderKey::AcceptRanges),
            "age" => Some(HeaderKey::Age),
            "etag" => Some(HeaderKey::ETag),
            "location" => Some(HeaderKey::Location),
            "proxy-authenticate" => Some(HeaderKey::ProxyAuthenticate),
            "retry-after" => Some(HeaderKey::RetryAfter),
            "server" => Some(HeaderKey::Server),
            "set-cookie" => Some(HeaderKey::SetCookie),
            "vary" => Some(HeaderKey::Vary),
            "www-authenticate" => Some(HeaderKey::WWWAuthenticate),

            // ===== Entity =====
            "allow" => Some(HeaderKey::Allow),
            "content-encoding" => Some(HeaderKey::ContentEncoding),
            "content-language" => Some(HeaderKey::ContentLanguage),
            "content-length" => Some(HeaderKey::ContentLength),
            "content-location" => Some(HeaderKey::ContentLocation),
            "content-range" => Some(HeaderKey::ContentRange),
            "content-type" => Some(HeaderKey::ContentType),
            "expires" => Some(HeaderKey::Expires),
            "last-modified" => Some(HeaderKey::LastModified),

            // ===== CORS / Fetch / Web =====
            "access-control-allow-credentials" => Some(HeaderKey::AccessControlAllowCredentials),
            "access-control-allow-headers" => Some(HeaderKey::AccessControlAllowHeaders),
            "access-control-allow-methods" => Some(HeaderKey::AccessControlAllowMethods),
            "access-control-allow-origin" => Some(HeaderKey::AccessControlAllowOrigin),
            "access-control-expose-headers" => Some(HeaderKey::AccessControlExposeHeaders),
            "access-control-max-age" => Some(HeaderKey::AccessControlMaxAge),

            "sec-fetch-dest" => Some(HeaderKey::SecFetchDest),
            "sec-fetch-mode" => Some(HeaderKey::SecFetchMode),
            "sec-fetch-site" => Some(HeaderKey::SecFetchSite),
            "sec-fetch-user" => Some(HeaderKey::SecFetchUser),

            // ===== WebSocket =====
            "sec-websocket-accept" => Some(HeaderKey::SecWebSocketAccept),
            "sec-websocket-extensions" => Some(HeaderKey::SecWebSocketExtensions),
            "sec-websocket-key" => Some(HeaderKey::SecWebSocketKey),
            "sec-websocket-protocol" => Some(HeaderKey::SecWebSocketProtocol),
            "sec-websocket-version" => Some(HeaderKey::SecWebSocketVersion),

            // ===== Proxy / Forwarded =====
            "forwarded" => Some(HeaderKey::Forwarded),
            "x-forwarded-for" => Some(HeaderKey::XForwardedFor),
            "x-forwarded-host" => Some(HeaderKey::XForwardedHost),
            "x-forwarded-proto" => Some(HeaderKey::XForwardedProto),

            // ===== Misc =====
            "dnt" => Some(HeaderKey::DNT),
            "keep-alive" => Some(HeaderKey::KeepAlive),
            "upgrade-insecure-requests" => Some(HeaderKey::UpgradeInsecureRequests),

            _ => None,
        }
    }
        /// 枚举转 &str
    pub fn to_str(&self) -> &'static str {
        HEADER_KEYS[*self as usize]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headerkey_to_str() {
        // 遍历所有枚举，保证 to_str 覆盖
        for i in 0u16..HEADER_KEYS.len() as u16 {
            let key = unsafe { std::mem::transmute::<u16, HeaderKey>(i) };
            let s = key.to_str();
            assert_eq!(s, HEADER_KEYS[i as usize]);
        }
    }

    #[test]
    fn test_headerkey_from_str_exact() {
        // 遍历所有 HEADER_KEYS，确保 from_str 可以匹配
        for i in 0..HEADER_KEYS.len() {
            let key_str = HEADER_KEYS[i];
            let key_enum = HeaderKey::from_str(key_str).unwrap();
            assert_eq!(key_enum.to_str(), key_str);

            // 测试大小写不敏感
            let key_enum_upper = HeaderKey::from_str(&key_str.to_uppercase()).unwrap();
            assert_eq!(key_enum_upper.to_str(), key_str);

            let key_enum_mixed = HeaderKey::from_str(&key_str.to_ascii_lowercase()).unwrap();
            assert_eq!(key_enum_mixed.to_str(), key_str);
        }
    }

    #[test]
    fn test_headerkey_from_str_invalid() {
        // 无效 header 返回 None
        assert!(HeaderKey::from_str("Invalid-Header").is_none());
        assert!(HeaderKey::from_str("").is_none());
        assert!(HeaderKey::from_str(" ").is_none());
        assert!(HeaderKey::from_str("123").is_none());
    }

    #[test]
    fn test_headerkey_all_from_to_roundtrip() {
        // 从枚举 -> str -> 枚举，保证完全一致
        for i in 0u16..HEADER_KEYS.len() as u16 {
            let key = unsafe { std::mem::transmute::<u16, HeaderKey>(i) };
            let s = key.to_str();
            let key_back = HeaderKey::from_str(s).unwrap();
            assert_eq!(key, key_back);
        }
    }
}
