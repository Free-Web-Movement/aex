use anyhow::Ok;
use futures::future::BoxFuture;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::Context;
use crate::connection::types::IDExtractor;
use crate::tcp::types::{Codec, TCPCommand, TCPFrame};

pub type Doer<F, C> = Box<
    dyn Fn(Arc<Mutex<Context>>, F, C) -> BoxFuture<'static, anyhow::Result<bool>>
        + Send
        + Sync
        + 'static,
>;

pub struct Router {
    pub handlers: HashMap<u32, Vec<Box<dyn Any + Send + Sync>>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// 修复语法：正确构建 Pin<Box<dyn Future>>
    pub fn on<F, C>(&mut self, key: u32, f: Doer<F, C>, middlewares: Vec<Doer<F, C>>)
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        // 1. 创建统一的线性链条
        // 预分配容量：middlewares 数量 + 1 个 executor
        let mut chain: Vec<Box<dyn Any + Send + Sync>> = Vec::with_capacity(middlewares.len() + 1);

        // 2. 先把所有中间件按顺序存入
        for mw in middlewares {
            chain.push(Box::new(mw));
        }

        // 3. 将最后的核心逻辑 f 存入
        chain.push(Box::new(f));

        // 4. 以 Any 类型持久化存储到 HashMap
        self.handlers.insert(key, chain);
    }

    /// 核心分发逻辑
    pub async fn handle_frame<F, C>(
        &self,
        ctx: Arc<Mutex<Context>>, // 假设你的 Context 定义是 Context<R, W>
        frame: F,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<bool>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        if !frame.validate() {
            return Ok(false); // 校验失败，跳过此帧
        }
        if let Some(data) = frame.command() {
            let key: u32;
            let c: Option<C>;
            if !frame.is_flat() {
                use std::result::Result::Ok;
                if let Ok(cmd) = <C as crate::tcp::types::Codec>::decode(&data) {
                    key = (extractor)(&cmd);
                    c = Some(cmd);
                } else {
                    let boxed_f = Box::new(frame.clone()) as Box<dyn Any>;
                    if let Ok(cmd) = boxed_f.downcast::<C>() {
                        let c_val = *cmd;
                        key = (extractor)(&c_val);
                        c = Some(c_val);
                    } else {
                        return Ok(false);
                    }
                }

                if let Some(any_handler) = self.handlers.get(&key) {
                    for handler in any_handler {
                        let handler = handler.downcast_ref::<Doer<F, C>>().ok_or_else(|| {
                            anyhow::anyhow!("Handler type mismatch for key: {}", key)
                        })?;
                        if !handler(ctx.clone(), frame.clone(), c.clone().unwrap().clone()).await? {
                            return Ok(false);
                        }
                    }
                }
            }
        }

        Ok(true)
    }

    pub async fn handle<F, C>(
        &self,
        ctx: Arc<Mutex<Context>>,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let mut session_buf: Vec<u8> = Vec::with_capacity(4096);
        let mut buf = vec![0u8; 1024];

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
                        let should_continue = self
                            .handle_frame::<F, C>(ctx.clone(), frame, extractor.clone())
                            .await?;

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
