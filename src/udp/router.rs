use std::any::Any;
use std::net::SocketAddr;
use std::pin::Pin;
use std::{collections::HashMap, sync::Arc};

use tokio::net::UdpSocket;

use crate::connection::global::GlobalContext;
use crate::connection::types::IDExtractor;
use crate::tcp::types::{Codec, Command, Frame};

pub struct Router {
    pub handlers: HashMap<u32, Box<dyn Any + Send + Sync>>,
}

pub type UdpHandler<C> = dyn Fn(
        Arc<GlobalContext>,
        C,
        SocketAddr,
        Arc<UdpSocket>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
    + Send
    + Sync;

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }
    // 注册 Handler 的方法与 TCP 类似，只需适配 PacketExecutor 的闭包即可
    pub fn on<C, FFut, Fut>(&mut self, key: u32, f: FFut)
    where
        C: Command + Send + Sync + 'static,
        FFut: Fn(Arc<GlobalContext>, C, SocketAddr, Arc<UdpSocket>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        // ⚡ 修正：直接构造 Box<UdpHandler<C>>
        let handler: Box<UdpHandler<C>> =
            Box::new(move |global, cmd, addr, socket| Box::pin(f(global, cmd, addr, socket)));

        // ⚡ 关键：直接把 handler 存入，不要再加一层 Box::new(...)
        // 这样 Any 里面存的就是 Box<UdpHandler<C>>
        self.handlers.insert(key, Box::new(handler));
    }

    pub async fn handle<F, C>(
        self: Arc<Self>,
        global: Arc<GlobalContext>,
        socket: Arc<UdpSocket>,
        extractor: IDExtractor<C>,
    ) -> anyhow::Result<()>
    where
        F: Frame + Send + Sync + 'static,
        C: Command + Send + Sync + 'static,
    {
        let mut buf = [0u8; 65535];
        loop {
            let (n, peer_addr) = socket.recv_from(&mut buf).await?;
            let data = buf[..n].to_vec();

            let router_ctx = self.clone();
            let socket_ctx = socket.clone();
            let extractor_ctx = extractor.clone();
            let global = global.clone();

            tokio::spawn(async move {
                // 1. Frame 解码与基础校验
                let frame = match <F as Codec>::decode(&data) {
                    Ok(f) if f.validate() => f,
                    _ => return,
                };

                let mut final_cmd: Option<C> = None;
                let mut key: u32 = 0;

                // 2. 开发者定义的路由分支：一级消息体 vs 二级消息体
                if frame.is_flat() {
                    // ⚡ 一级消息体：Frame 实例直接下转为 Command
                    let boxed_f = Box::new(frame) as Box<dyn Any>;
                    if let Ok(cmd) = boxed_f.downcast::<C>() {
                        let c_val = *cmd;
                        key = (extractor_ctx)(&c_val);
                        final_cmd = Some(c_val);
                    } else {
                        eprintln!("[UDP Error] Flat frame downcast to Command failed");
                        return;
                    }
                } else {
                    // ⚡ 二级消息体：从 Frame 中剥离 Payload 并解码
                    if let Some(payload) = frame.command() {
                        if let Ok(cmd) = <C as Codec>::decode(&payload) {
                            if cmd.validate() {
                                key = (extractor_ctx)(&cmd);
                                final_cmd = Some(cmd);
                            }
                        }
                    }
                }

                // 3. 路由分发 (执行业务 Handler)
                if let Some(cmd) = final_cmd {
                    if let Some(any_handler) = router_ctx.handlers.get(&key) {
                        if let Some(handler) = any_handler.downcast_ref::<Box<UdpHandler<C>>>() {
                            if let Err(e) = handler(global, cmd, peer_addr, socket_ctx).await {
                                eprintln!("[UDP Handler Error]: {:?}", e);
                            }
                        }
                    }
                }
            });
        }
    }
}
