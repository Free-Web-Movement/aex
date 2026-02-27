use std::collections::HashMap;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;
use std::future::Future;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::tcp::types::{Command, Frame};

// 假设这些在你之前的定义中
// use crate::tcp::types::{Codec, Frame, Command, RawCodec, frame_config};

/// ⚡ 修复后的 Handler 签名：使用 BoxFuture 确保异步闭包可用
pub type CommandHandler<C> = Box<dyn Fn(
    C, 
    Box<dyn AsyncRead + Unpin + Send>, 
    Box<dyn AsyncWrite + Unpin + Send>
) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>> + Send + Sync>;

pub struct Router<F, C, K = u32> 
where 
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static,
    K: Eq + Hash + Send + Sync + 'static 
{
    pub handlers: HashMap<K, CommandHandler<C>>,
    // 这里的 extractor 将 Command 映射为路由 Key
    pub extractor: Arc<dyn Fn(&C) -> K + Send + Sync>,
    _phantom: std::marker::PhantomData<F>,
}

impl<F, C, K> Router<F, C, K> 
where 
    F: Frame + Send + Sync + 'static,
    C: Command + Send + Sync + 'static,
    K: Eq + Hash + Send + Sync + 'static 
{
    pub fn new(extractor: impl Fn(&C) -> K + Send + Sync + 'static) -> Self {
        Self {
            handlers: HashMap::new(),
            extractor: Arc::new(extractor),
            _phantom: std::marker::PhantomData,
        }
    }

    /// 修复语法：正确构建 Pin<Box<dyn Future>>
pub fn on<FFut, Fut>(&mut self, key: K, f: FFut)
where
    FFut: Fn(C, Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
{
    self.handlers.insert(
        key, 
        Box::new(move |cmd, r, w| Box::pin(f(cmd, r, w)))
    );
}

    /// 核心分发逻辑
    pub async fn handle_frame(
        &self,
        frame: F,
        reader: &mut Option<OwnedReadHalf>,
        writer: &mut Option<OwnedWriteHalf>,
    ) -> anyhow::Result<bool> {
        // 1. 调用 Frame 的验证逻辑
        if !frame.validate() {
            return Ok(true); // 校验失败，跳过此帧
        }

        // 2. 剥壳获取 Payload
        if let Some(data) = frame.handle() {
            // 3. 使用你固定的 Codec::decode 恢复 Command 对象
            if let Ok(cmd) = <C as crate::tcp::types::Codec>::decode(&data) {
                // 逻辑校验
                if !cmd.validate() { return Ok(true); }

                let key = (self.extractor)(&cmd);
                
                if let Some(handler) = self.handlers.get(&key) {
                    // 转移 IO 句柄所有权
                    let r = reader.take().ok_or_else(|| anyhow::anyhow!("Reader already taken"))?;
                    let w = writer.take().ok_or_else(|| anyhow::anyhow!("Writer already taken"))?;
                    
                    // 执行业务 Handler
                    return handler(cmd, Box::new(r), Box::new(w)).await;
                }
            }
        }

        Ok(true)
    }
}