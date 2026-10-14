use bytes::Bytes;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::Context;
use crate::connection::global::FrameErrorPolicy;
use crate::constants::tcp::MAX_FRAME_SIZE;

/// A registered TCP handler.
///
/// Receives the shared connection context plus the raw `frame` and `command`
/// bytes. The framing/splitting is performed by the router's [`Extractor`], so a
/// handler only decodes the representation it cares about.
pub type Doer = Box<
    dyn Fn(Arc<Mutex<Context>>, Bytes, Bytes) -> BoxFuture<'static, anyhow::Result<bool>>
        + Send
        + Sync
        + 'static,
>;

/// Splits an accumulated byte buffer into a single frame.
///
/// Returns `Ok(Some((consumed, key, frame_bytes, command_bytes)))` when one
/// frame is available, `Ok(None)` when more bytes are needed, and `Err` when the
/// bytes are positively **not** this router's protocol (for example a frame that
/// decodes but fails validation) so the caller stops trying.
pub type Extractor = Arc<
    dyn Fn(&[u8]) -> anyhow::Result<Option<(usize, u32, Bytes, Bytes)>> + Send + Sync + 'static,
>;

/// Codec-agnostic TCP router.
///
/// The router works exclusively on [`Bytes`]; protocol framing/decoding is
/// supplied by the caller through [`Router::extractor`]. The dispatch key is the
/// `u32` returned by the extractor.
pub struct Router {
    pub handlers: HashMap<u32, Vec<Doer>>,
    extractor: Option<Extractor>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            extractor: None,
        }
    }

    /// Install the byte-level extractor that frames the stream and derives the
    /// dispatch key. Replaces the former `Router<F, C>` type parameters.
    pub fn extractor<E>(mut self, extractor: E) -> Self
    where
        E: Fn(&[u8]) -> anyhow::Result<Option<(usize, u32, Bytes, Bytes)>> + Send + Sync + 'static,
    {
        self.extractor = Some(Arc::new(extractor));
        self
    }

    pub fn get_extractor(&self) -> Option<&Extractor> {
        self.extractor.as_ref()
    }

    pub fn on(&mut self, key: u32, f: Doer, middlewares: Vec<Doer>) {
        let mut chain = Vec::with_capacity(middlewares.len() + 1);
        for mw in middlewares {
            chain.push(mw);
        }
        chain.push(f);
        self.handlers.insert(key, chain);
    }

    /// Convenience: register a handler without middlewares.
    pub fn on_simple(&mut self, key: u32, f: Doer) {
        self.on(key, f, Vec::new());
    }

    /// Handle a single, already-complete frame.
    pub async fn handle_frame(
        &self,
        ctx: Arc<Mutex<Context>>,
        frame: &[u8],
    ) -> anyhow::Result<bool> {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TCP extractor not set"))?;
        let policy = ctx.lock().await.global.frame_error_policy;
        match extractor(frame) {
            Ok(Some((_consumed, key, frame_bytes, cmd_bytes))) => {
                self.dispatch(ctx, key, frame_bytes, cmd_bytes).await
            }
            Ok(None) => Ok(false),
            Err(e) => match policy {
                FrameErrorPolicy::Drop => {
                    tracing::warn!("dropping invalid frame: {e}");
                    Ok(false)
                }
                FrameErrorPolicy::Close => Err(e),
            },
        }
    }

    async fn dispatch(
        &self,
        ctx: Arc<Mutex<Context>>,
        key: u32,
        frame_bytes: Bytes,
        cmd_bytes: Bytes,
    ) -> anyhow::Result<bool> {
        if let Some(chain) = self.handlers.get(&key) {
            for f in chain {
                if !f(ctx.clone(), frame_bytes.clone(), cmd_bytes.clone()).await? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub async fn handle(&self, ctx: Arc<Mutex<Context>>) -> anyhow::Result<()> {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TCP extractor not set"))?;

        let mut buf = vec![0u8; MAX_FRAME_SIZE];
        let mut session_buf: Vec<u8> = Vec::with_capacity(MAX_FRAME_SIZE + 4096);

        // 💡 核心优化：在 I/O 时不锁定 Context，解决 P2P 双向传递锁死问题
        let (reader, policy) = {
            let mut guard = ctx.lock().await;
            (guard.reader.take(), guard.global.frame_error_policy)
        };

        let mut r = match reader {
            Some(r) => r,
            None => return Ok(()),
        };

        loop {
            // 在不锁定 Context 的情况下进行异步读取
            let n = match r.read(&mut buf).await {
                std::result::Result::Ok(n) => n,
                std::result::Result::Err(e) => {
                    let mut guard = ctx.lock().await;
                    guard.reader = Some(r);
                    return Err(e.into());
                }
            };

            if n == 0 {
                break;
            }
            crate::connection::metrics::record_global_received(n as u64);
            session_buf.extend_from_slice(&buf[..n]);

            // 正确处理粘包与半包
            loop {
                match extractor(&session_buf) {
                    Ok(Some((consumed, key, frame_bytes, cmd_bytes))) => {
                        let cont = self
                            .dispatch(ctx.clone(), key, frame_bytes, cmd_bytes)
                            .await?;
                        session_buf.drain(..consumed);
                        if !cont {
                            let mut guard = ctx.lock().await;
                            guard.reader = Some(r);
                            return Ok(());
                        }
                    }
                    Ok(None) => break,
                    Err(e) => match policy {
                        FrameErrorPolicy::Drop => {
                            tracing::warn!("dropping invalid frame, keeping connection: {e}");
                            session_buf.clear();
                            break;
                        }
                        FrameErrorPolicy::Close => {
                            let mut guard = ctx.lock().await;
                            guard.reader = Some(r);
                            return Err(e);
                        }
                    },
                }
            }
        }

        let mut guard = ctx.lock().await;
        guard.reader = Some(r);
        Ok(())
    }
}
