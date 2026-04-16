//! # AEX Constants
//!
//! Unified constants for all protocols, limits, and characters.

pub mod http {
    //! HTTP Protocol Constants

    pub const MAX_CAPACITY: i32 = 1024;
    pub const TIME_LIMIT_MS: i32 = 500;
    pub const MAX_REQUEST_LINE_SIZE: usize = 4096;
    pub const MAX_HEADER_SIZE: usize = 8192;
    pub const MAX_HEADER_COUNT: usize = 64;
    pub const MAX_COOKIE_COUNT: usize = 32;
    pub const MAX_FORM_BODY_SIZE: usize = 65536;

    pub const HTTP_VERSION: &str = "HTTP/1.1";
    pub const HEADER_DELIMITER: &str = "\r\n";
    pub const HEADER_KV_DELIMITER: &str = ": ";
    pub const LINE_DELIMITER: &[u8] = b"\r\n";
    pub const HEADER_END: &[u8] = b"\r\n\r\n";

    pub const CONTENT_LENGTH_KEY: &str = "Content-Length";
    pub const CONTENT_TYPE_KEY: &str = "Content-Type";
    pub const COOKIE_KEY: &str = "Cookie";
    pub const SET_COOKIE_KEY: &str = "Set-Cookie";
    pub const LOCATION_KEY: &str = "Location";
    pub const HOST_KEY: &str = "Host";
    pub const USER_AGENT_KEY: &str = "User-Agent";
    pub const ACCEPT_KEY: &str = "Accept";
    pub const ORIGIN_KEY: &str = "Origin";

    pub const STATUS_OK: u16 = 200;
    pub const STATUS_BAD_REQUEST: u16 = 400;
    pub const STATUS_NOT_FOUND: u16 = 404;
    pub const STATUS_FOUND: u16 = 302;
}

pub mod tcp {
    //! TCP Protocol Constants

    pub const MAX_FRAME_SIZE: usize = 65536;
    pub const MAX_HANDSHAKE_SIZE: usize = 4096;
    pub const PROTOCOL_HEADER_SIZE: usize = 8;

    pub const HANDSHAKE_VERSION: u8 = 1;
    pub const DEFAULT_PING_INTERVAL_SEC: u64 = 30;
    pub const DEFAULT_PING_TIMEOUT_SEC: u64 = 10;
}

pub mod udp {
    //! UDP Protocol Constants

    pub const DEFAULT_MULTICAST_TTL: u8 = 64;
    pub const MAX_UDP_PACKET_SIZE: usize = 65507;
}

pub mod server {
    //! Server Constants

    pub const SERVER_NAME: &str = "Aex/1.0";
    pub const DEFAULT_APP_DIR: &str = ".aex";
    pub const DEFAULT_PORT: u16 = 8080;
    pub const MAX_CONNECTIONS: usize = 1024;
    pub const BACKLOG_SIZE: u32 = 128;
}

pub mod protocol {
    //! Protocol Flags & Common Constants

    pub const NONE: u8 = 0b0000_0000;
    pub const COMPRESSED: u8 = 0b0000_0001;
    pub const ENCRYPTED: u8 = 0b0000_0010;
    pub const PRIORITY: u8 = 0b0000_0100;
    pub const FRAGMENT: u8 = 0b0000_1000;

    pub const PARAM_PREFIX: char = ':';
    pub const WILDCARD: char = '*';
    pub const PATH_SEPARATOR: char = '/';
    pub const QUERY_PREFIX: char = '?';
    pub const FRAGMENT_PREFIX: char = '#';

    pub const EMPTY_STRING: &str = "";
    pub const DEFAULT_CHARSET: &str = "utf-8";
}

pub mod codec {
    //! Binary Codec Constants

    pub const BINCODE_MAX_SIZE: usize = 65536;
    pub const VARINT_MAX_BYTES: usize = 10;
    pub const UUID_BYTES: usize = 16;
    pub const TIMESTAMP_BYTES: usize = 8;
}
