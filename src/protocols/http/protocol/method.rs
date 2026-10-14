use tokio::io::AsyncBufReadExt;

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET = 0,
    HEAD,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH,
    PROPFIND,
    PROPPATCH,
    MKCOL,
    MKCALENDAR,
    COPY,
    MOVE,
    LOCK,
    UNLOCK,
    SEARCH,
    PURGE,
    LINK,
    UNLINK,
}

pub const HTTP_METHODS: [&str; 21] = [
    "GET",
    "HEAD",
    "POST",
    "PUT",
    "DELETE",
    "CONNECT",
    "OPTIONS",
    "TRACE",
    "PATCH",
    "PROPFIND",
    "PROPPATCH",
    "MKCOL",
    "MKCALENDAR",
    "COPY",
    "MOVE",
    "LOCK",
    "UNLOCK",
    "SEARCH",
    "PURGE",
    "LINK",
    "UNLINK",
];

impl HttpMethod {
    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        // zero-allocation case-insensitive matching
        if s.len() < 3 || s.len() > 10 {
            return None;
        }
        match s {
            s if s.eq_ignore_ascii_case("GET") => Some(HttpMethod::GET),
            s if s.eq_ignore_ascii_case("HEAD") => Some(HttpMethod::HEAD),
            s if s.eq_ignore_ascii_case("POST") => Some(HttpMethod::POST),
            s if s.eq_ignore_ascii_case("PUT") => Some(HttpMethod::PUT),
            s if s.eq_ignore_ascii_case("DELETE") => Some(HttpMethod::DELETE),
            s if s.eq_ignore_ascii_case("CONNECT") => Some(HttpMethod::CONNECT),
            s if s.eq_ignore_ascii_case("OPTIONS") => Some(HttpMethod::OPTIONS),
            s if s.eq_ignore_ascii_case("TRACE") => Some(HttpMethod::TRACE),
            s if s.eq_ignore_ascii_case("PATCH") => Some(HttpMethod::PATCH),
            s if s.eq_ignore_ascii_case("PROPFIND") => Some(HttpMethod::PROPFIND),
            s if s.eq_ignore_ascii_case("PROPPATCH") => Some(HttpMethod::PROPPATCH),
            s if s.eq_ignore_ascii_case("MKCOL") => Some(HttpMethod::MKCOL),
            s if s.eq_ignore_ascii_case("MKCALENDAR") => Some(HttpMethod::MKCALENDAR),
            s if s.eq_ignore_ascii_case("COPY") => Some(HttpMethod::COPY),
            s if s.eq_ignore_ascii_case("MOVE") => Some(HttpMethod::MOVE),
            s if s.eq_ignore_ascii_case("LOCK") => Some(HttpMethod::LOCK),
            s if s.eq_ignore_ascii_case("UNLOCK") => Some(HttpMethod::UNLOCK),
            s if s.eq_ignore_ascii_case("SEARCH") => Some(HttpMethod::SEARCH),
            s if s.eq_ignore_ascii_case("PURGE") => Some(HttpMethod::PURGE),
            s if s.eq_ignore_ascii_case("LINK") => Some(HttpMethod::LINK),
            s if s.eq_ignore_ascii_case("UNLINK") => Some(HttpMethod::UNLINK),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::HEAD => "HEAD",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::CONNECT => "CONNECT",
            HttpMethod::OPTIONS => "OPTIONS",
            HttpMethod::TRACE => "TRACE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::PROPFIND => "PROPFIND",
            HttpMethod::PROPPATCH => "PROPPATCH",
            HttpMethod::MKCOL => "MKCOL",
            HttpMethod::MKCALENDAR => "MKCALENDAR",
            HttpMethod::COPY => "COPY",
            HttpMethod::MOVE => "MOVE",
            HttpMethod::LOCK => "LOCK",
            HttpMethod::UNLOCK => "UNLOCK",
            HttpMethod::SEARCH => "SEARCH",
            HttpMethod::PURGE => "PURGE",
            HttpMethod::LINK => "LINK",
            HttpMethod::UNLINK => "UNLINK",
        }
    }

    /// 判断一段字符串是否以合法 HTTP Method 开头
    #[inline]
    pub fn is_prefixed(s: &str) -> bool {
        // 找到第一个空格，HTTP 请求行一定是 "METHOD SP ..."
        let method = match s.find(' ') {
            Some(pos) => &s[..pos],
            None => {
                return false;
            }
        };

        HttpMethod::from_str(method).is_some()
    }

    #[inline]
    pub fn is_prefixed_bytes(buf: &[u8]) -> bool {
        for &method in HTTP_METHODS.iter() {
            let m = method.as_bytes();
            if buf.len() > m.len() && buf[m.len()] == b' ' && buf[..m.len()].eq_ignore_ascii_case(m)
            {
                return true;
            }
        }
        false
    }

    pub async fn is_http_connection<R>(reader: &mut R) -> anyhow::Result<bool>
    where
        R: tokio::io::AsyncBufRead + Unpin + ?Sized,
    {
        // fill_buf() 会返回当前缓冲区的数据，但不会移动读取位置
        // 这在逻辑上等同于一次成功的 peek
        let buf = reader.fill_buf().await?;

        if buf.is_empty() {
            return Ok(false);
        }

        // 取前 16 个字节进行 HTTP 前缀判定
        let limit = std::cmp::min(buf.len(), 16);
        let s = std::str::from_utf8(&buf[..limit]).unwrap_or("");

        Ok(HttpMethod::is_prefixed(s))
    }
}
