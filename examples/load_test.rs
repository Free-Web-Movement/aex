//! 轻量 HTTP 压测客户端：测 req/s。
//! usage: cargo run --example load_test -- <base_url> [concurrency] [total]

use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: load_test <base_url> [concurrency] [total]"))?;
    let concurrency: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "8".into())
        .parse()?;
    let total: u64 = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "20000".into())
        .parse()?;

    let client = reqwest::Client::new();
    let paths = ["/", "/api/users", "/api/users/123"];

    let start = Instant::now();
    let per_worker = total / concurrency as u64;
    let mut handles = Vec::new();
    for _ in 0..concurrency {
        let client = client.clone();
        let base = url.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..per_worker {
                let p = paths[(i as usize) % paths.len()];
                let res = client
                    .get(format!("{}{}", base, p))
                    .send()
                    .await
                    .expect("request failed");
                let _ = res.bytes().await;
            }
        }));
    }
    for h in handles {
        h.await?;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let done = per_worker * concurrency as u64;
    println!(
        "{} reqs in {:.3}s = {:.0} req/s",
        done,
        elapsed,
        done as f64 / elapsed
    );
    Ok(())
}
