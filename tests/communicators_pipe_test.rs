#[cfg(test)]
mod tests {
    use std::sync::{ Arc, atomic::{ AtomicUsize, Ordering } };
    use aex::communicators::pipe::PipeManager;
    use futures::future::FutureExt;

    #[tokio::test]
    async fn test_pipe_register_and_send_success() {
        let manager = PipeManager::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        // 1. 注册接收端：收到数字后将其累加到计数器
        manager
            .register(
                "add_one",
                Box::new(move |val: usize| {
                    let c = Arc::clone(&counter_clone);
                    (
                        async move {
                            c.fetch_add(val, Ordering::SeqCst);
                        }
                    ).boxed()
                })
            ).await
            .expect("Register should succeed");

        // 2. 发送端投递：模拟多个生产者发送数据
        manager.send("add_one", 10usize).await.expect("Send 1 should succeed");
        manager.send("add_one", 20usize).await.expect("Send 2 should succeed");

        // 3. 给异步任务一点处理时间
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 30);
    }

    #[tokio::test]
    async fn test_pipe_conflict_error() {
        let manager = PipeManager::new();

        // 第一次注册成功
        let res1 = manager.register(
            "unique_pipe",
            Box::new(|_: String| (async {}).boxed())
        ).await;
        assert!(res1.is_ok());

        // 第二次注册同名管道，应该返回 Err
        let res2 = manager.register(
            "unique_pipe",
            Box::new(|_: String| (async {}).boxed())
        ).await;
        assert!(res2.is_err());
        assert!(res2.unwrap_err().contains("already in use"));
    }

    #[tokio::test]
    async fn test_pipe_type_mismatch() {
        let manager = PipeManager::new();

        // 注册为处理 String 的管道
        manager
            .register(
                "string_pipe",
                Box::new(|_: String| (async {}).boxed())
            ).await
            .unwrap();

        // 尝试发送 i32 类型，应该返回类型不匹配错误
        let res = manager.send("string_pipe", 123i32).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Type mismatch"));
    }

    #[tokio::test]
    async fn test_pipe_not_found() {
        let manager = PipeManager::new();

        // 直接向未注册的管道发送数据
        let res = manager.send("ghost_pipe", "hello".to_string()).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not registered"));
    }

    #[tokio::test]
    async fn test_pipe_registration_race_condition_fixed() {
        let manager = Arc::new(PipeManager::new());
        let pipe_name = "race_pipe";

        // 增加并发密度：一次启动 10 个任务
        let num_tasks = 10;
        let barrier = Arc::new(tokio::sync::Barrier::new(num_tasks));
        let mut handles = Vec::new();

        for _ in 0..num_tasks {
            let mgr = Arc::clone(&manager);
            let bar = Arc::clone(&barrier);
            handles.push(
                tokio::spawn(async move {
                    bar.wait().await;
                    mgr.register(
                        pipe_name,
                        Box::new(|_: String| (async {}).boxed())
                    ).await
                })
            );
        }

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.unwrap());
        }

        let ok_count = results
            .iter()
            .filter(|r| r.is_ok())
            .count();
        let err_count = results
            .iter()
            .filter(|r| r.is_err())
            .count();

        assert_eq!(ok_count, 1);
        assert_eq!(err_count, num_tasks - 1);

        // 只要有一个触发了 race condition，就说明逻辑覆盖到了
        let has_race_msg = results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .any(|e| e.contains("conflict during race condition"));

        // 注意：在高负载下，可能所有失败者都卡在第一步，也可能有人冲进第二步
        // 这里我们至少验证了并发安全性
        println!("是否存在竞态冲突报错: {}", has_race_msg);
    }
}
