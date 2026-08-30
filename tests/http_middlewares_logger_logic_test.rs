use aex::{
    connection::{context::Context, global::GlobalContext},
    http::{meta::HttpMetadata, middlewares::logger::LogConfig},
};
use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct TestWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl io::Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestWriter {
    type Writer = TestWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port).parse().unwrap()
}

fn make_ctx(port: u16) -> Context {
    let a = addr(port);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    ctx.local.set_value(HttpMetadata::new());
    ctx
}

fn capture() -> (Arc<Mutex<Vec<u8>>>, tracing::dispatcher::DefaultGuard) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = TestWriter { buf: buf.clone() };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (buf, guard)
}

fn log_text(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock().unwrap()).to_string()
}

#[tokio::test]
async fn logger_logs_method_and_path() {
    let (buf, _guard) = capture();
    let mut ctx = make_ctx(9200);
    assert!(LogConfig::new().all().build()(&mut ctx).await);
    let out = log_text(&buf);
    assert!(out.contains("GET / [AEX]"), "log was: {out}");
}

#[tokio::test]
async fn logger_method_only_logs_single_token() {
    let (buf, _guard) = capture();
    let mut ctx = make_ctx(9201);
    assert!(LogConfig::new().log_method(true).build()(&mut ctx).await);
    let out = log_text(&buf);
    assert!(out.contains("GET [AEX]"), "log was: {out}");
    assert!(!out.contains("GET / [AEX]"), "log was: {out}");
}

#[tokio::test]
async fn logger_path_only_logs_path() {
    let (buf, _guard) = capture();
    let mut ctx = make_ctx(9202);
    assert!(LogConfig::new().log_path(true).build()(&mut ctx).await);
    let out = log_text(&buf);
    assert!(out.contains("/ [AEX]"), "log was: {out}");
}

#[tokio::test]
async fn logger_default_logs_nothing() {
    let (buf, _guard) = capture();
    let mut ctx = make_ctx(9203);
    assert!(LogConfig::new().build()(&mut ctx).await);
    assert!(!log_text(&buf).contains("[AEX]"));
}

#[tokio::test]
async fn logger_without_metadata_passes_through() {
    let a = addr(9204);
    let global = Arc::new(GlobalContext::new(a, None));
    let mut ctx = Context::new(None, None, global, a);
    assert!(LogConfig::new().all().build()(&mut ctx).await);
}

#[tokio::test]
async fn logger_macro_builds_working_middleware() {
    let (buf, _guard) = capture();
    let mut ctx = make_ctx(9205);
    assert!(aex::logger!()(&mut ctx).await);
    assert!(log_text(&buf).contains("GET / [AEX]"));
}
