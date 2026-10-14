//! # HTTP Module
//!
//! Core HTTP web framework components.
//!
//! ## Components
//!
//! - `router`: Trie-tree based HTTP router
//! - `types`: Executor type definition
//! - `meta`: HTTP request/response metadata
//! - `req`: Request parsing
//! - `res`: Response handling
//! - `params`: URL path/query/form parameters
//! - `websocket`: WebSocket support
//! - `macros`: HTTP method macros (get!, post!, etc.)
//! - `middlewares`: Built-in middleware implementations
//! - `static_files`: Static file serving with MIME auto-detection
//! - `protocol`: HTTP protocol types (method, status, headers, etc.)

pub mod macros;
pub mod meta;
pub mod middlewares;
pub mod params;
pub mod protocol;
pub mod req;
pub mod res;
pub mod router;
pub mod static_files;
pub mod types;
pub mod websocket;
