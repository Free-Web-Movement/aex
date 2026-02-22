use tokio::io::AsyncWriteExt;

use crate::{connection::context::{SharedWriter, TypeMap}, http::protocol::status::StatusCode};

pub struct Response<'a, W> {
    pub writer: &'a SharedWriter<W>,
    pub local: &'a TypeMap,
}

impl<'a, W> Response<'a, W> 
where 
    W: AsyncWriteExt + Unpin 
{
    /// 发送响应，会自动竞争 writer 锁
    pub async fn send_status(&mut self, status: StatusCode) -> anyhow::Result<()> {
        let mut w = self.writer.lock().await;
        let res = format!("HTTP/1.1 {} {}\r\n\r\n", status as u16, status.to_str());
        w.write_all(res.as_bytes()).await?;
        w.flush().await?;
        Ok(())
    }
}