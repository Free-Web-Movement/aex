use std::fmt;
use std::hash::Hasher;
use std::hash::Hash;

macro_rules! define_header_keys {
    ($($name:ident => $string:expr),* $(,)?) => {
        #[derive(Debug, Clone)] // 注意：去掉了 Eq, PartialEq, Hash 的 derive
        pub enum HeaderKey {
            $($name,)*
            Custom(String),
        }

        impl HeaderKey {
            pub fn from_str(s: &str) -> Option<Self> {
                let s_trimmed = s.trim();
                let s_lower = s_trimmed.to_ascii_lowercase();
                match s_lower.as_str() {
                    $(
                        s if s == $string.to_ascii_lowercase() => Some(HeaderKey::$name),
                    )*
                    // 存储原始格式，不再强制小写
                    _ => Some(HeaderKey::Custom(s_trimmed.to_string())),
                }
            }

            pub fn as_str(&self) -> &str {
                match self {
                    $(
                        HeaderKey::$name => $string,
                    )*
                    HeaderKey::Custom(s) => s.as_str(),
                }
            }
        }

        // --- 手动实现比较逻辑 ---

        impl PartialEq for HeaderKey {
            fn eq(&self, other: &Self) -> bool {
                // 仅在比较时转为小写，不影响存储
                self.as_str().to_ascii_lowercase() == other.as_str().to_ascii_lowercase()
            }
        }

        impl Eq for HeaderKey {}

        impl Hash for HeaderKey {
            fn hash<H: Hasher>(&self, state: &mut H) {
                // 仅在计算哈希时转为小写，确保不同大小写的输入在 HashMap 中落在同一个槽位
                self.as_str().to_ascii_lowercase().hash(state);
            }
        }
    };
}
// 使用宏统一管理所有 70+ 个标准 Header
define_header_keys! {
    // ===== General Headers =====
    CacheControl => "Cache-Control",
    Connection => "Connection",
    Date => "Date",
    Pragma => "Pragma",
    Trailer => "Trailer",
    TransferEncoding => "Transfer-Encoding",
    Upgrade => "Upgrade",
    Via => "Via",
    Warning => "Warning",

    // ===== Request Headers =====
    Accept => "Accept",
    AcceptCharset => "Accept-Charset",
    AcceptEncoding => "Accept-Encoding",
    AcceptLanguage => "Accept-Language",
    Authorization => "Authorization",
    Cookie => "Cookie",
    Expect => "Expect",
    From => "From",
    Host => "Host",
    IfMatch => "If-Match",
    IfModifiedSince => "If-Modified-Since",
    IfNoneMatch => "If-None-Match",
    IfRange => "If-Range",
    IfUnmodifiedSince => "If-Unmodified-Since",
    MaxForwards => "Max-Forwards",
    Origin => "Origin",
    Range => "Range",
    Referer => "Referer",
    TE => "TE",
    UserAgent => "User-Agent",

    // ===== Response Headers =====
    AcceptRanges => "Accept-Ranges",
    Age => "Age",
    ETag => "ETag",
    Location => "Location",
    ProxyAuthenticate => "Proxy-Authenticate",
    RetryAfter => "Retry-After",
    Server => "Server",
    SetCookie => "Set-Cookie",
    Vary => "Vary",
    WWWAuthenticate => "WWW-Authenticate",

    // ===== Entity Headers =====
    Allow => "Allow",
    ContentEncoding => "Content-Encoding",
    ContentLanguage => "Content-Language",
    ContentLength => "Content-Length",
    ContentLocation => "Content-Location",
    ContentRange => "Content-Range",
    ContentType => "Content-Type",
    Expires => "Expires",
    LastModified => "Last-Modified",

    // ===== CORS / Fetch =====
    AccessControlAllowCredentials => "Access-Control-Allow-Credentials",
    AccessControlAllowHeaders => "Access-Control-Allow-Headers",
    AccessControlAllowMethods => "Access-Control-Allow-Methods",
    AccessControlAllowOrigin => "Access-Control-Allow-Origin",
    AccessControlExposeHeaders => "Access-Control-Expose-Headers",
    AccessControlMaxAge => "Access-Control-Max-Age",

    SecFetchDest => "Sec-Fetch-Dest",
    SecFetchMode => "Sec-Fetch-Mode",
    SecFetchSite => "Sec-Fetch-Site",
    SecFetchUser => "Sec-Fetch-User",

    // ===== WebSocket =====
    SecWebSocketAccept => "Sec-WebSocket-Accept",
    SecWebSocketExtensions => "Sec-WebSocket-Extensions",
    SecWebSocketKey => "Sec-WebSocket-Key",
    SecWebSocketProtocol => "Sec-WebSocket-Protocol",
    SecWebSocketVersion => "Sec-WebSocket-Version",

    // ===== Proxy =====
    Forwarded => "Forwarded",
    XForwardedFor => "X-Forwarded-For",
    XForwardedHost => "X-Forwarded-Host",
    XForwardedProto => "X-Forwarded-Proto",

    // ===== Misc =====
    DNT => "DNT",
    KeepAlive => "Keep-Alive",
    UpgradeInsecureRequests => "Upgrade-Insecure-Requests",
}

impl fmt::Display for HeaderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}