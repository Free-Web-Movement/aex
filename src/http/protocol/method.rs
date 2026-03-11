use tokio::{io::AsyncBufReadExt, net::tcp::OwnedReadHalf};

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
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Some(HttpMethod::GET),
            "HEAD" => Some(HttpMethod::HEAD),
            "POST" => Some(HttpMethod::POST),
            "PUT" => Some(HttpMethod::PUT),
            "DELETE" => Some(HttpMethod::DELETE),
            "CONNECT" => Some(HttpMethod::CONNECT),
            "OPTIONS" => Some(HttpMethod::OPTIONS),
            "TRACE" => Some(HttpMethod::TRACE),
            "PATCH" => Some(HttpMethod::PATCH),
            "PROPFIND" => Some(HttpMethod::PROPFIND),
            "PROPPATCH" => Some(HttpMethod::PROPPATCH),
            "MKCOL" => Some(HttpMethod::MKCOL),
            "MKCALENDAR" => Some(HttpMethod::MKCALENDAR), // <-- 对应新增
            "COPY" => Some(HttpMethod::COPY),
            "MOVE" => Some(HttpMethod::MOVE),
            "LOCK" => Some(HttpMethod::LOCK),
            "UNLOCK" => Some(HttpMethod::UNLOCK),
            "SEARCH" => Some(HttpMethod::SEARCH),
            "PURGE" => Some(HttpMethod::PURGE),
            "LINK" => Some(HttpMethod::LINK),
            "UNLINK" => Some(HttpMethod::UNLINK),
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
