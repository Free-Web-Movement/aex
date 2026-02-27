#[cfg(test)]
mod tests {
    use aex::communicators::event::{ Event, EventEmitter };
    use futures::FutureExt;
    use std::sync::{ Arc, atomic::{ AtomicUsize, Ordering } };

    #[tokio::test]
    async fn test_event_emitter_complex() {
        let emitter = EventEmitter::new();
        let string_call_count = Arc::new(AtomicUsize::new(0));

        // 注意这里增加了 .await
        let s_count = Arc::clone(&string_call_count);

        emitter.on(
            "update".to_string(),
            Arc::new(move |_data: String| {
                let c = Arc::clone(&s_count);
                (
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await;

        emitter.notify("update".to_string(), "test".to_string()).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(string_call_count.load(Ordering::SeqCst), 1);
    }
}
