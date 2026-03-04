use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::tcp::types::{Codec, Command, Frame};

// 假设这些在你之前的定义中
// use crate::tcp::types::{Codec, Frame, Command, RawCodec, frame_config};

/// ⚡ 修复后的 Handler 签名：使用 BoxFuture 确保异步闭包可用
pub type CommandHandler<F, C> = dyn Fn(
        Arc<GlobalContext>,
        &mut F,
        &mut C,
        Box<dyn AsyncRead + Unpin + Send>,
        Box<dyn AsyncWrite + Unpin + Send>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
    + Send
    + Sync;

pub struct Router {
    pub handlers: HashMap<u32, Box<dyn Any + Send + Sync>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// 修复语法：正确构建 Pin<Box<dyn Future>>
    pub fn on<F, C, FFut, Fut>(&mut self, key: u32, f: FFut)
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
        FFut: Fn(
                Arc<GlobalContext>,
                &mut F,
                &mut C,
                Box<dyn AsyncRead + Unpin + Send>,
                Box<dyn AsyncWrite + Unpin + Send>,
            ) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        // ⚡ 这里的 handler 类型是 Box<CommandHandler<C>>
        let handler: Box<CommandHandler<F, C>> =
            Box::new(move |global, frame, cmd, r, w| Box::pin(f(global, frame, cmd, r, w)));

        // ⚡ 最小修改：直接存入这个 Box。
        self.handlers.insert(key, Box::new(handler));
    }

    /// 核心分发逻辑
    pub async fn handle_frame<F, C>(
        &self,
        global: Arc<GlobalContext>,
        frame: F,
        reader: &mut Option<OwnedReadHalf>,
        writer: &mut Option<OwnedWriteHalf>,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<bool>
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
    {
        if !frame.validate() {
            return Ok(false); // 校验失败，跳过此帧
        }
        if let Some(data) = frame.command() {
            let key: u32;
            let c: Option<C>;
            if !frame.is_flat() {
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
                    // ⚡ 最小修改：匹配 on 存入的 Box<Box<...>> 结构
                    let handler = any_handler
                        .downcast_ref::<Box<CommandHandler<F, C>>>()
                        .ok_or_else(|| anyhow::anyhow!("Handler type mismatch for key: {}", key))?;

                    // ⚡ 最小修改：修复错误字符串以适配单元测试断言
                    let r = reader
                        .take()
                        .ok_or_else(|| anyhow::anyhow!("Reader already taken"))?;
                    let w = writer
                        .take()
                        .ok_or_else(|| anyhow::anyhow!("Writer already taken"))?;

                    match c {
                        Some(mut cmd) => {
                            return handler(global, &mut frame.clone(), &mut cmd, Box::new(r), Box::new(w)).await;
                        }
                        None => {
                            eprintln!("Command not implemented!");
                        }
                    }
                }
            }
        }

        Ok(true)
    }

    pub async fn handle<F, C>(
        &self,
        global: Arc<GlobalContext>,
        reader: OwnedReadHalf,
        writer: OwnedWriteHalf,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<()>
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
    {
        let mut r_opt = Some(reader);
        let mut w_opt = Some(writer);

        let mut session_buf: Vec<u8> = Vec::with_capacity(4096); // ⚡ 优化：持有未处理数据的缓冲区
        // 固定的轻量级缓冲区，仅用于读取 Frame 头
        let mut buf = vec![0u8; 1024];
        println!("inside handle!");

        loop {
            println!("inside loop!");

            let r = match r_opt.as_mut() {
                Some(r) => r,
                None => break,
            };

            let n = r.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            session_buf.extend_from_slice(&buf[..n]); // ⚡ 将新读到的数据追加到缓冲区

            // ⚡ 循环尝试解码，直到缓冲区数据不足以构成一个完整 Frame
            while !session_buf.is_empty() {
                match <F as Codec>::decode(&session_buf) {
                    Ok(frame) => {
                        // 计算已消费长度（这需要你的 Codec 提供已读长度，
                        // 如果没有，假设 decode 消费了全部。但标准做法是返回 (Frame, consumed_bytes)）
                        // 临时简单处理：假设每次 decode 一个完整 Frame 后清空（单包模式）
                        let should_continue = self
                            .handle_frame(
                                global.clone(),
                                frame,
                                &mut r_opt,
                                &mut w_opt,
                                extractor.clone(),
                            )
                            .await?;

                        session_buf.clear(); // ⚡ 生产环境应根据 consumed_bytes 移除：session_buf.drain(..len);

                        if !should_continue || r_opt.is_none() {
                            println!("inside ok!");
                            return Ok(());
                        }
                    }
                    Err(_) => {
                        // ⚡ 关键：如果是 UnexpectedEnd (长度不足)，跳出 while 继续 read
                        println!("inside ok!");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}
