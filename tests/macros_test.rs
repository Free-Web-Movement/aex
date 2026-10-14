#[cfg(test)]
mod tests {
    use aex::bytes::Bytes;
    use aex::tcp::router::Router;
    use futures::future::BoxFuture;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn ok_handler(
        _ctx: Arc<Mutex<aex::connection::context::Context>>,
        _frame: Bytes,
        _cmd: Bytes,
    ) -> BoxFuture<'static, anyhow::Result<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn no_op_extractor(_buf: &[u8]) -> anyhow::Result<Option<(usize, u32, Bytes, Bytes)>> {
        Ok(None)
    }

    #[test]
    fn router_macro_builds_router_with_handlers() {
        let router = aex::router!(no_op_extractor, {
            1u32 => Box::new(ok_handler),
            2u32 => Box::new(ok_handler),
        });
        assert_eq!(router.handlers.len(), 2);
        assert!(router.get_extractor().is_some());
    }

    #[test]
    fn router_new_via_type_is_empty() {
        let router = Router::new();
        assert_eq!(router.handlers.len(), 0);
    }
}
