use std::collections::HashMap;
use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::tcp::types::Command;

/// Handler 签名：处理指令，并可以直接操作 reader/writer
/// 返回 bool：true 继续服务，false 断开连接
pub type CommandHandler<C> = Box<dyn Fn(
    C,                             // 当前指令
    Box<dyn AsyncRead + Unpin + Send>, // 抽象的 Reader
    Box<dyn AsyncWrite + Unpin + Send> // 抽象的 Writer
) -> BoxFuture<'static, bool> + Send + Sync>;

pub struct Router<C: Command> {
    pub handlers: HashMap<u32, CommandHandler<C>>,
}

impl<C: Command> Router<C> {
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// 核心 API：注册指令处理器
    pub fn on<F, Fut>(&mut self, id: u32, f: F)
    where
        F: Fn(C, Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        self.handlers.insert(id, Box::new(move |cmd, r, w| Box::pin(f(cmd, r, w))));
    }
}