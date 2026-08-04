//! # Static File Serving
//!
//! Serve files from a directory with automatic MIME detection, suitable for
//! basic websites (HTML/CSS/JS/images). Use via `Router::static_files`:
//!
//! ```rust,ignore
//! router.static_files("/static", "public");
//! // GET /static/app.js -> public/app.js (application/javascript)
//! // GET /static        -> public/index.html (text/html)
//! // GET /static/sub    -> directory listing (when no index.html)
//! ```
//!
//! ## Directory listing
//!
//! When a directory has no index file, an HTML listing of its contents is
//! generated (like `python -m http.server` / nginx autoindex). Entries show a
//! **distinct per-extension icon** (folder 📁, JS 🟨, TS 🔷, Python 🐍, Rust 🦀,
//! image 🖼️/🌅, text 📄/📝, audio 🎵, video 🎬, archive 📦/🗜️, PDF 📕, ...)
//! plus file size; subdirectories are linked so you can navigate into them
//! recursively. A directory is served only if the process has permission to
//! read it.
//!
//! ## Size limit
//!
//! Files larger than [`DEFAULT_MAX_FILE_SIZE`] (100 MiB) are rejected with
//! `404 Not Found`, so a huge file cannot tie up the server. For larger
//! downloads, consider a dedicated download service (streaming with
//! Content-Length / Range support).

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::connection::context::Context;
use crate::http::protocol::media_type::SubMediaType;
use crate::http::protocol::status::StatusCode;
use crate::http::types::Executor;

/// 默认单文件大小上限：100 MiB。覆盖内网常见的几十 M 级文件下载；
/// 超过上限的文件请走专门的下载服务。
pub const DEFAULT_MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// 静态文件服务配置。
pub struct StaticFiles {
    dir: PathBuf,
    max_file_size: u64,
    index: String,
}

impl StaticFiles {
    /// 从根目录构建静态文件服务。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            index: "index.html".to_string(),
        }
    }

    /// 自定义单文件大小上限（字节）。默认 100 MiB。
    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }

    /// 自定义目录入口文件名（默认 `index.html`）。没有入口文件时，
    /// 总是生成目录列表页（不可关闭），子目录可递归进入。
    pub fn index(mut self, file: &str) -> Self {
        self.index = file.to_string();
        self
    }

    /// 构建静态文件 Executor。
    pub fn build(self) -> Arc<Executor> {
        let dir = self.dir;
        let max = self.max_file_size;
        let index = self.index;
        Arc::new(move |ctx: &mut Context| {
            let dir = dir.clone();
            let index = index.clone();
            Box::pin(async move { serve(&dir, max, &index, ctx).await })
        })
    }
}

/// 默认 404 响应。
fn not_found(ctx: &mut Context) -> bool {
    ctx.status(StatusCode::NotFound).text("Not Found");
    false
}

/// 解析并校验目标路径：解析符号链接后必须仍位于根目录之内，
/// 否则返回 `None`（防止 symlink 等访问到根目录之外的文件）。
async fn resolve_in_root(root: &Path, target: &Path) -> Option<PathBuf> {
    let resolved = tokio::fs::canonicalize(target).await.ok()?;
    if resolved.starts_with(root) {
        Some(resolved)
    } else {
        None
    }
}

/// 读取并响应单个静态文件；目录则回退到入口文件（默认 index.html），
/// 没有入口文件时生成目录列表页（不可关闭）。
async fn serve(dir: &Path, max: u64, index: &str, ctx: &mut Context) -> bool {
    let rel = ctx.req().param("*").unwrap_or_default();
    let rel = rel.trim_start_matches('/');

    // 路径穿越防护：拒绝任何 ".." 段。
    if rel.split(['/', '\\']).any(|c| c == "..") {
        return not_found(ctx);
    }

    // 根目录归一化；根目录本身不可解析则拒绝服务。
    let root = match tokio::fs::canonicalize(dir).await {
        Ok(root) => root,
        Err(_) => return not_found(ctx),
    };

    let target = root.join(rel);

    // 符号链接解析 + 越界校验：访问目标必须仍在根目录之内。
    let resolved = match resolve_in_root(&root, &target).await {
        Some(p) => p,
        None => return not_found(ctx),
    };

    let meta = match tokio::fs::metadata(&resolved).await {
        Ok(meta) => meta,
        Err(_) => return not_found(ctx),
    };

    // 目录：优先入口文件，没有则输出目录列表页（可递归进入）。
    if meta.is_dir() {
        // 目录统一重定向加尾部斜杠（301），保证相对链接（../ 与子目录）解析正确，
        // 与 nginx/apache 的目录行为一致。
        let path = ctx.req().path().to_string();
        if !path.ends_with('/') {
            let location = match path.split_once('?') {
                Some((base, query)) => format!("{base}/?{query}"),
                None => format!("{path}/"),
            };
            ctx.redirect(&location);
            ctx.status(StatusCode::MovedPermanently);
            return true;
        }
        let index_file = resolved.join(index);
        if tokio::fs::metadata(&index_file).await.is_ok() {
            return match resolve_in_root(&root, &index_file).await {
                Some(index_resolved) => serve_file(ctx, &index_resolved, max).await,
                None => not_found(ctx),
            };
        }
        return serve_dir_listing(ctx, &resolved, !rel.is_empty()).await;
    }

    serve_file(ctx, &resolved, max).await
}

/// 读取并响应单个普通文件；目录、超限、不可读均拒绝。
async fn serve_file(ctx: &mut Context, file: &Path, max: u64) -> bool {
    let meta = match tokio::fs::metadata(file).await {
        Ok(meta) => meta,
        Err(_) => return not_found(ctx),
    };

    // 不是普通文件，或超过大小上限：拒绝。
    if !meta.is_file() || meta.len() > max {
        return not_found(ctx);
    }

    match tokio::fs::read(file).await {
        Ok(bytes) => {
            let mime = SubMediaType::guess(file);
            ctx.send_bytes(bytes, Some(mime));
            true
        }
        Err(_) => not_found(ctx),
    }
}

/// 生成目录列表页（类似 `python -m http.server`）。
/// `has_parent`：是否显示 `../` 上级链接（服务前缀根目录不显示）。
async fn serve_dir_listing(ctx: &mut Context, dir: &Path, has_parent: bool) -> bool {
    let mut entries: Vec<(String, bool, u64)> = Vec::new();
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return not_found(ctx),
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let (is_dir, size) = match entry.metadata().await {
            Ok(meta) => (meta.is_dir(), if meta.is_file() { meta.len() } else { 0 }),
            Err(_) => (false, 0),
        };
        entries.push((name, is_dir, size));
    }
    // 目录优先，随后按名称（忽略大小写）排序。
    entries.sort_by(|a, b| {
        let da = a.1 as u8;
        let db = b.1 as u8;
        da.cmp(&db)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

    let req_path = ctx.req().path().to_string();
    let display_path = req_path.trim_end_matches('/');
    let display_path = if display_path.is_empty() {
        "/"
    } else {
        display_path
    };

    let mut html = String::with_capacity(2048);
    html.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>Index of ");
    html.push_str(&html_escape(display_path));
    html.push_str("</title>\n<style>");
    html.push_str("body{font-family:system-ui,sans-serif;margin:2em auto;max-width:60em;padding:0 1em;color:#333}");
    html.push_str("h1{font-size:1.4em;border-bottom:1px solid #ddd;padding-bottom:.4em}");
    html.push_str("ul{list-style:none;padding:0}li{padding:.35em .5em;display:flex;justify-content:space-between}");
    html.push_str(
        "li:hover{background:#f5f5f5}a{color:#0366d6;text-decoration:none;word-break:break-all}",
    );
    html.push_str(
        ".icon{width:1.5em;display:inline-block;text-align:center;margin-right:.35em;flex:none}",
    );
    html.push_str(".name{flex:1}");
    html.push_str(".size{color:#888;font-size:.85em;margin-left:1em;white-space:nowrap}");
    html.push_str("</style>\n</head>\n<body>\n<h1>Index of ");
    html.push_str(&html_escape(display_path));
    html.push_str("</h1>\n<ul>\n");
    if has_parent {
        html.push_str("<li><span class=\"icon\">");
        html.push_str(file_icon("..", true));
        html.push_str(
            "</span><a class=\"name\" href=\"../\">../</a><span class=\"size\"></span></li>\n",
        );
    }
    for (name, is_dir, size) in entries {
        let suffix = if is_dir { "/" } else { "" };
        let href = url_encode_path(&name) + suffix;
        let icon = file_icon(&name, is_dir);
        let size_text = if is_dir {
            String::new()
        } else {
            human_size(size)
        };
        html.push_str("<li><span class=\"icon\">");
        html.push_str(icon);
        html.push_str("</span><a class=\"name\" href=\"");
        html.push_str(&html_escape(&href));
        html.push_str("\">");
        html.push_str(&html_escape(&name));
        html.push_str(&html_escape(suffix));
        html.push_str("</a><span class=\"size\">");
        html.push_str(&html_escape(&size_text));
        html.push_str("</span></li>\n");
    }
    html.push_str("</ul>\n</body>\n</html>\n");

    ctx.send_bytes(html.into_bytes(), Some(SubMediaType::Html));
    true
}

/// HTML 特殊字符转义。
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 对 URL 路径段做百分号编码（保留 ASCII 字母/数字/`-_.~`）。
fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{:02X}", b);
        }
    }
    out
}

/// 人类可读的文件大小。
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// 目录/文件类型对应的 emoji 图标，用于目录列表页直观识别类型。
/// 目录固定为文件夹图标；文件按扩展名给出**专属图标**，一次性覆盖
/// 主流语言、文档、图片、音视频、压缩包、配置/数据、字体、3D、密钥等
/// 上百种扩展名，后期一般无需再改。
fn file_icon(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "📁";
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        // 网页与模板
        "html" | "htm" | "jsp" | "asp" | "aspx" | "cshtml" | "hbs" | "mustache" | "ejs" | "pug"
        | "erb" | "twig" | "jinja" => "🌐",
        "css" | "scss" | "sass" | "less" | "styl" => "🎨",
        "js" | "mjs" | "cjs" => "🟨",
        "jsx" | "tsx" => "⚛️",
        "ts" => "🔷",
        "vue" => "💚",
        "svelte" => "🧡",
        "wasm" => "🧩",
        "map" => "🗺️",

        // 配置与数据
        "json" | "json5" | "jsonc" | "ndjson" | "jsonl" => "🧾",
        "yaml" | "yml" | "editorconfig" => "🗂️",
        "toml" | "ini" | "cfg" | "conf" | "properties" => "🛠️",
        "env" => "🔐",
        "lock" => "🔒",
        "sql" => "🗄️",
        "db" | "sqlite" | "sqlite3" | "mdb" | "accdb" => "🗄️",
        "parquet" | "arrow" | "feather" => "📊",
        "xml" | "xslt" | "xsd" | "dtd" | "rss" | "atom" => "📋",
        "csv" | "tsv" => "📊",
        "gpx" | "kml" => "🗺️",

        // 构建与部署
        "makefile" | "mk" | "cmake" | "gradle" | "pom" | "tf" | "hcl" => "🏗️",
        "dockerfile" => "🐳",
        "nix" => "❄️",

        // 文本与文档
        "md" | "markdown" | "org" => "📝",
        "txt" | "rst" => "📄",
        "log" => "🗒️",
        "tex" | "sty" | "cls" | "bib" => "🎓",
        "pdf" => "📕",
        "epub" => "📚",
        "doc" | "docx" | "rtf" | "odt" => "📃",
        "xls" | "xlsx" | "ods" => "📗",
        "ppt" | "pptx" | "odp" => "📽️",

        // 图片
        "png" => "🖼️",
        "jpg" | "jpeg" => "🌅",
        "gif" | "apng" => "🎞️",
        "webp" | "bmp" | "avif" | "tiff" | "tif" | "heic" | "heif" => "🖼️",
        "svg" => "📐",
        "ico" => "🖼️",
        "raw" | "cr2" | "nef" | "arw" | "dng" => "📷",
        "psd" | "psb" | "xcf" => "🖌️",
        "ai" => "🖌️",
        "dxf" | "dwg" => "📐",

        // 音频
        "mp3" => "🎵",
        "wav" | "ogg" | "oga" | "m4a" | "aac" | "opus" | "amr" | "wma" => "🎵",
        "flac" | "wv" | "alac" => "🎼",
        "mid" | "midi" => "🎹",

        // 视频与字幕
        "mp4" | "m4v" | "mkv" | "webm" | "flv" | "wmv" | "mov" | "mts" | "m2ts" | "mpg"
        | "mpeg" | "vob" | "3gp" | "ogv" => "🎬",
        "avi" => "🎥",
        "srt" | "vtt" | "ass" | "ssa" | "sub" => "💬",

        // 压缩包与安装包
        "zip" => "📦",
        "tar" | "cab" => "🗃️",
        "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz" | "lz" | "lz4" | "zst" | "zstd" => "🗜️",
        "iso" | "img" => "💿",
        "dmg" => "💽",
        "jar" | "war" | "ear" | "deb" | "rpm" => "📦",
        "gem" => "💎",
        "apk" => "🤖",
        "ipa" => "🍎",
        "exe" | "msi" | "bin" | "elf" | "com" => "⚙️",
        "app" => "🖥️",
        "dll" | "so" | "dylib" | "a" | "lib" | "o" => "🔗",
        "bat" | "cmd" | "ps1" => "🪟",

        // 脚本与源码语言
        "sh" | "bash" | "zsh" | "fish" | "ksh" => "🐚",
        "py" | "pyw" => "🐍",
        "ipynb" => "📓",
        "r" => "📈",
        "rs" => "🦀",
        "go" => "🐹",
        "c" => "🅲",
        "h" => "🅷",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => "🅲",
        "cs" => "🆑",
        "vb" | "vbs" => "🅱",
        "java" | "class" | "coffee" => "☕",
        "rb" => "💎",
        "php" => "🐘",
        "swift" => "🕊️",
        "kt" | "kts" => "🅺",
        "scala" => "🆂",
        "groovy" | "clj" | "cljs" | "cljc" => "🌀",
        "elixir" | "ex" | "exs" => "💧",
        "erl" | "hrl" => "🧵",
        "haskell" | "hs" | "lhs" => "🦄",
        "fs" | "fsi" => "🅵",
        "ml" | "mli" => "🦉",
        "lisp" | "lsp" | "cl" | "el" | "scheme" | "scm" | "rkt" | "awk" => "🧮",
        "pl" | "pm" | "raku" => "🦙",
        "lua" => "🌙",
        "dart" => "🎯",
        "julia" | "jl" => "🌻",
        "zig" => "⚡",
        "nim" => "🅽",
        "v" => "🅥",
        "d" => "🅳",
        "crystal" => "💠",
        "m" => "🅼",
        "graphql" | "gql" | "prisma" => "🟪",
        "proto" | "thrift" | "avro" => "📡",

        // 字体 / 3D / 地图
        "ttf" | "otf" | "woff" | "woff2" | "eot" | "fon" => "🔤",
        "stl" | "obj" | "gltf" | "glb" | "fbx" | "ply" | "blend" | "3ds" | "max" | "step"
        | "stp" | "iges" | "x3d" => "🧊",

        // 思维导图 / 种子 / 密钥
        "mm" | "xmind" => "🧠",
        "torrent" => "🧲",
        "key" | "pem" | "crt" | "cer" | "der" | "p12" | "pfx" | "p7b" | "csr" | "jks" | "pub" => {
            "🔑"
        }
        "sig" | "gpg" | "asc" => "🔏",

        _ => "📄",
    }
}
