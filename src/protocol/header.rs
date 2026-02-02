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


