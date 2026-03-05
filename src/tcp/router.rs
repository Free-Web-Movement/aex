use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncReadExt, AsyncWrite};

use crate::connection::context::Context;
use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::tcp::types::{Codec, Command, Frame};

// 假设这些在你之前的定义中
// use crate::tcp::types::{Codec, Frame, Command, RawCodec, frame_config};

/// ⚡ 修复后的 Handler 签名：使用 BoxFuture 确保异步闭包可用
pub type CommandHandler<F, C> = dyn Fn(&mut Context<'_>, &mut F, &mut C) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
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
        FFut: Fn(&mut Context<'_>, &mut F, &mut C) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        // ⚡ 这里的 handler 类型是 Box<CommandHandler<C>>
        let handler: Box<CommandHandler<F, C>> =
            Box::new(move |ctx, frame, cmd| Box::pin(f(ctx, frame, cmd)));

        // ⚡ 最小修改：直接存入这个 Box。
        self.handlers.insert(key, Box::new(handler));
    }

    /// 核心分发逻辑
    pub async fn handle_frame<F, C>(
        &self,
        ctx: &mut Context<'_>, // 假设你的 Context 定义是 Context<R, W>
        frame: F,
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
                    let handler = any_handler
                        .downcast_ref::<Box<CommandHandler<F, C>>>()
                        .ok_or_else(|| anyhow::anyhow!("Handler type mismatch for key: {}", key))?;

                    // ⚡ 关键：使用 take() 拿走所有权，留下 None
                    // 这样你就得到了 Box<dyn AsyncBufRead + Send + Unpin>
                    // let r = reader
                    //     .take()
                    //     .ok_or_else(|| anyhow::anyhow!("Reader already taken"))?;
                    // let w = writer
                    //     .take()
                    //     .ok_or_else(|| anyhow::anyhow!("Writer already taken"))?;

                    // 现在 r 和 w 正好符合 Handler 的参数要求
                    let result = handler(ctx, &mut frame.clone(), &mut c.unwrap()).await?;

                    // 如果你希望在 Handler 执行完后继续在 loop 中使用这些流，
                    // Handler 必须在返回值中把它们还回来（见下文建议）。
                    return Ok(result);
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
        reader: &mut Option<Box<dyn AsyncBufRead + Unpin + Send>>,
        writer: &mut Option<Box<dyn AsyncWrite + Unpin + Send>>,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<()>
    where
        F: Frame + Send + Sync + Clone + 'static,
        C: Command + Send + Sync + 'static,
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
                match <F as Codec>::decode(&session_buf) {
                    Ok(frame) => {
                        // ⚡ 修复点 2：直接透传外部传入的 reader/writer (它们已经是 Option)
                        // handle_frame 内部会使用 reader.take()
                        // let mut reader: Option<Box<dyn AsyncBufRead + Send + Unpin>> =
                        //     Some(Box::new(reader));
                        // let mut writer: Option<Box<dyn AsyncWrite + Send + Unpin>> =
                        //     Some(Box::new(writer));

                        let mut ctx = Context::new(reader, writer, global.clone(), addr);
                        let should_continue = self
                            .handle_frame::<F, C>(&mut ctx, frame, extractor.clone())
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
