use anyhow::Ok;
use futures::future::BoxFuture;
use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::connection::context::{BoxReader, BoxWriter, Context};
use crate::connection::global::GlobalContext;
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
                    // ... 之前的 decode 成功逻辑 ...
                    key = (extractor)(&cmd);
                    c = Some(cmd);
                } else {
                    // 只有在 decode 失败且满足 F == C 时才尝试直接转换
                    let boxed_f = Box::new(frame.clone()) as Box<dyn Any>;
                    if let Ok(cmd) = boxed_f.downcast::<C>() {
                        let c_val = *cmd; // 这里拿到了 C 的所有权
                        key = (extractor)(&c_val);
                        c = Some(c_val);
                    } else {
                        // 如果既不能 decode 也不是同类型，处理报错或跳过
                        return Ok(false);
                    }
                }

                if let Some(any_handler) = self.handlers.get(&key) {
                    for handler in any_handler {
                        let handler =
                            handler.downcast_ref::<Box<Doer<F, C>>>().ok_or_else(|| {
                                anyhow::anyhow!("Handler type mismatch for key: {}", key)
                            })?;

                        // 现在 r 和 w 正好符合 Handler 的参数要求
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
        addr: SocketAddr,
        global: Arc<GlobalContext>,
        // ⚡ 直接传入 Option 的 mutable 引用，这样 handle_frame 才能 take 走并放回
        reader: &mut Option<BoxReader>,
        writer: &mut Option<BoxWriter>,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<()>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        let mut session_buf: Vec<u8> = Vec::with_capacity(4096);
        let mut buf = vec![0u8; 1024];
        println!("inside handle!");

        loop {
            // ⚡ 修复点 1：解包 Option 拿到里面的 Box (它才实现了 AsyncBufRead)
            // 使用 as_deref_mut() 拿到 &mut (dyn AsyncBufRead + ...)
            let n = match &mut reader.as_deref_mut() {
                Some(r) => r.read(&mut buf).await?,
                None => {
                    println!("Reader taken and not returned!");
                    break;
                }
            };

            if n == 0 {
                break;
            }
            session_buf.extend_from_slice(&buf[..n]);

            while !session_buf.is_empty() {
                use std::result::Result::Ok;
                match <F as Codec>::decode(&session_buf) {
                    Ok(frame) => {
                        let ctx = Context::new(reader.take(), writer.take(), global.clone(), addr);
                        let should_continue = self
                            .handle_frame::<F, C>(
                                Arc::new(Mutex::new(ctx)),
                                frame,
                                extractor.clone(),
                            )
                            .await?;

                        session_buf.clear();

                        // ⚡ 修复点 3：检查 reader 是否被 Handler 归还
                        if !should_continue || reader.is_none() {
                            println!("Handler terminated connection or kept the stream");
                            return Ok(());
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        Ok(())
    }
}