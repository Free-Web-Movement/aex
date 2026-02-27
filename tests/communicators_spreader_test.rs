use std::sync::Arc;

use aex::communicators::spreader::SpreadManager;
use futures::FutureExt;

#[tokio::test]
async fn test_spread_broadcast() {
    let spread = SpreadManager::new();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // 订阅者 1
    let c1 = Arc::clone(&counter);
    spread
        .subscribe(
            "config_update",
            Box::new(move |v: usize| {
                let c = Arc::clone(&c1);
                (
                    async move {
                        c.fetch_add(v, std::sync::atomic::Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await
        .unwrap();

    // 订阅者 2
    let c2 = Arc::clone(&counter);
    spread
        .subscribe(
            "config_update",
            Box::new(move |v: usize| {
                let c = Arc::clone(&c2);
                (
                    async move {
                        c.fetch_add(v, std::sync::atomic::Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await
        .unwrap();

    // 发布一次消息，两个订阅者都应该收到
    spread.publish("config_update", 10usize).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // 10 + 10 = 20
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 20);
}
