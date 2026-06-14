use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::Context;
use crate::constants::tcp::MAX_FRAME_SIZE;
use crate::tcp::types::{Codec, TCPCommand, TCPFrame};

pub type Doer<F, C> = Box<
    dyn Fn(Arc<Mutex<Context>>, F, C) -> BoxFuture<'static, anyhow::Result<bool>>
        + Send
        + Sync
        + 'static,
>;

pub struct Router<F = (), C = ()> {
    pub handlers: HashMap<u32, Vec<Doer<F, C>>>,
    extractor: Option<Arc<dyn Fn(&C) -> u32 + Send + Sync>>,
    _phantom: std::marker::PhantomData<(F, C)>,
}

impl<F, C> Router<F, C> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            extractor: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn extractor<E: Fn(&C) -> u32 + Send + Sync + 'static>(mut self, extractor: E) -> Self {
        self.extractor = Some(Arc::new(extractor));
        self
    }

    pub fn get_extractor(&self) -> Option<&Arc<dyn Fn(&C) -> u32 + Send + Sync>> {
        self.extractor.as_ref()
    }

    pub fn on(&mut self, key: u32, f: Doer<F, C>, middlewares: Vec<Doer<F, C>>) {
        let mut chain = Vec::with_capacity(middlewares.len() + 1);
        for mw in middlewares {
            chain.push(mw);
        }
        chain.push(f);
        self.handlers.insert(key, chain);
    }

    /// Convenience: register a handler without middlewares.
    pub fn on_simple(&mut self, key: u32, f: Doer<F, C>) {
        self.on(key, f, Vec::new());
    }

    /// Handle a decoded frame using stored handlers.
    pub async fn handle_frame(&self, ctx: Arc<Mutex<Context>>, frame: F) -> anyhow::Result<bool>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TCP extractor not set"))?;
        if !frame.validate() {
            return Ok(false);
        }
        if let Some(data) = frame.command() {
            let key: u32;
            let c: Option<C>;
            if !frame.is_flat() {
                if let Ok(cmd) = <C as Codec>::decode(&data) {
                    key = (extractor)(&cmd);
                    c = Some(cmd);
                } else {
                    return Ok(false);
                }

                if let Some(any_handler) = self.handlers.get(&key) {
                    for handler in any_handler {
                        if !handler(ctx.clone(), frame.clone(), c.clone().unwrap()).await? {
                            return Ok(false);
                        }
                    }
                }
            }
        }

        Ok(true)
    }

    pub async fn handle(&self, ctx: Arc<Mutex<Context>>) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let _ = self
            .extractor
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TCP extractor not set"))?;

        let mut session_buf: Vec<u8> = Vec::with_capacity(MAX_FRAME_SIZE + 4096);
        let mut buf = vec![0u8; MAX_FRAME_SIZE];

        // 💡 核心优化：在 I/O 时不锁定 Context，解决 P2P 双向传递锁死问题
        let reader = {
            let mut guard = ctx.lock().await;
            guard.reader.take()
        };

        if reader.is_none() {
            return Ok(());
        }
        let mut r = reader.unwrap();

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
            session_buf.extend_from_slice(&buf[..n]);

            // 正确处理粘包与半包
            while !session_buf.is_empty() {
                match <F as Codec>::decode_with_len(&session_buf) {
                    std::result::Result::Ok((frame, consumed)) => {
                        let should_continue = self.handle_frame(ctx.clone(), frame).await?;

                        session_buf.drain(0..consumed);

                        if !should_continue {
                            let mut guard = ctx.lock().await;
                            guard.reader = Some(r);
                            return std::result::Result::Ok(());
                        }
                    }
                    std::result::Result::Err(_) => {
                        // 等待更多数据
                        break;
                    }
                }
            }
        }

        // 归还 Reader
        let mut guard = ctx.lock().await;
        guard.reader = Some(r);
        std::result::Result::Ok(())
    }
}
