//! Router Performance Benchmark
//!
//! Run with: cargo run --release --example router_benchmark
//!
//! This benchmark compares AEX's Trie router against:
//! 1. std::HashMap - Standard library hash map
//! 2. ahash::AHashMap - Fast, non-cryptographic hash map
//! 3. Actix-web / Axum - Pattern matching simulation
//! 5. AEX Trie - Pattern-matching router

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use aex::exe;
use aex::http::params::SmallParams;
use aex::http::router::{NodeType, Router};
use aex::http::types::Executor;
use ahash::AHashMap;

fn create_aex_router() -> Router {
    let mut router = Router::new(NodeType::Static("root".into()));
    let handler: Arc<Executor> = exe!(|_ctx| { true });
    router.get("/api/users", handler.clone()).register();
    router.get("/api/users/:id", handler.clone()).register();
    router
        .get("/api/users/:id/posts", handler.clone())
        .register();
    router.post("/api/users", handler.clone()).register();
    router.get("/api/posts", handler.clone()).register();
    router.get("/api/posts/:slug", handler.clone()).register();
    router
        .get("/api/posts/:slug/comments/:id", handler.clone())
        .register();
    router.get("/health", handler.clone()).register();
    router.get("/", handler.clone()).register();
    router.get("/about", handler.clone()).register();
    router.get("/contact", handler.clone()).register();
    router
}

fn benchmark_aex_fast_path<'a>(routes: &[(&'a str, Vec<&'a str>)]) -> f64 {
    let router = create_aex_router();
    const ITERATIONS: usize = 10_000_000;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        for (_, segs) in routes {
            black_box(router.match_route_fast(segs));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * routes.len()) as f64
}

fn benchmark_aex_param<'a>(routes: &[(&'a str, Vec<&'a str>)]) -> f64 {
    let router = create_aex_router();
    const ITERATIONS: usize = 5_000_000;
    let start = Instant::now();
    let mut params = SmallParams::new();
    for _ in 0..ITERATIONS {
        for (_, segs) in routes {
            params.clear();
            black_box(router.match_route(segs, &mut params));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * routes.len()) as f64
}

fn benchmark_ahash_static() -> f64 {
    let mut routes: AHashMap<String, ()> = AHashMap::new();
    routes.insert("/api/users".to_string(), ());
    routes.insert("/api/posts".to_string(), ());
    routes.insert("/health".to_string(), ());
    routes.insert("/".to_string(), ());
    routes.insert("/about".to_string(), ());
    routes.insert("/contact".to_string(), ());
    let test_paths: Vec<String> = vec![
        "/api/users".to_string(),
        "/api/posts".to_string(),
        "/health".to_string(),
        "/".to_string(),
        "/about".to_string(),
        "/contact".to_string(),
    ];
    const ITERATIONS: usize = 10_000_000;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        for path in &test_paths {
            black_box(routes.contains_key(path.as_str()));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * test_paths.len()) as f64
}

fn benchmark_std_hashmap_static() -> f64 {
    use std::collections::HashMap;
    let mut routes: HashMap<String, ()> = HashMap::new();
    routes.insert("/api/users".to_string(), ());
    routes.insert("/api/posts".to_string(), ());
    routes.insert("/health".to_string(), ());
    routes.insert("/".to_string(), ());
    routes.insert("/about".to_string(), ());
    routes.insert("/contact".to_string(), ());
    let test_paths: Vec<String> = vec![
        "/api/users".to_string(),
        "/api/posts".to_string(),
        "/health".to_string(),
        "/".to_string(),
        "/about".to_string(),
        "/contact".to_string(),
    ];
    const ITERATIONS: usize = 10_000_000;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        for path in &test_paths {
            black_box(routes.contains_key(path.as_str()));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * test_paths.len()) as f64
}

fn benchmark_framework_static<F>(routes: &[&str], mut matcher: F) -> f64
where
    F: FnMut(&str) -> bool,
{
    const ITERATIONS: usize = 10_000_000;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        for route in routes {
            black_box(matcher(route));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * routes.len()) as f64
}

fn benchmark_framework_param<F>(routes: &[&str], mut matcher: F) -> f64
where
    F: FnMut(&str) -> Option<Vec<(&str, &str)>>,
{
    const ITERATIONS: usize = 5_000_000;
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        for route in routes {
            black_box(matcher(route));
        }
    }
    let elapsed = start.elapsed();
    elapsed.as_nanos() as f64 / (ITERATIONS * routes.len()) as f64
}

fn main() {
    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║           Router Performance Benchmark - Rust Web Frameworks             ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    println!("║  Comparing: AEX vs Actix-web vs Axum vs HashMap                       ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    println!("Building routers...");
    let _aex_router = create_aex_router();

    let static_routes = [
        ("/api/users", vec!["api", "users"]),
        ("/api/posts", vec!["api", "posts"]),
        ("/health", vec!["health"]),
        ("/", vec![]),
        ("/about", vec!["about"]),
        ("/contact", vec!["contact"]),
    ];

    let param_routes = [
        ("/api/users/123", vec!["api", "users", "123"]),
        ("/api/users/456/posts", vec!["api", "users", "456", "posts"]),
        ("/api/posts/my-post", vec!["api", "posts", "my-post"]),
        (
            "/api/posts/another-post/comments/789",
            vec!["api", "posts", "another-post", "comments", "789"],
        ),
        ("/api/users/999", vec!["api", "users", "999"]),
    ];

    println!("Running benchmarks...\n");

    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("                         STATIC ROUTE BENCHMARK (ns/op)                       ");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  {:25} {:>12} {:>12}", "Framework", "ns/op", "vs Best");
    println!("─────────────────────────────────────────────────────────────────────────────");

    let std_ns = benchmark_std_hashmap_static();
    let ahash_ns = benchmark_ahash_static();

    let actix_ns = benchmark_framework_static(
        &static_routes.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        |path| {
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            match segs.as_slice() {
                [] => path == "/",
                ["api", "users"] => true,
                ["api", "posts"] => true,
                ["health"] => true,
                ["about"] => true,
                ["contact"] => true,
                _ => false,
            }
        },
    );

    let axum_ns = benchmark_framework_static(
        &static_routes.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        |path| {
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            match segs.as_slice() {
                [] => path == "/",
                ["api", "users"] => true,
                ["api", "posts"] => true,
                ["health"] => true,
                ["about"] => true,
                ["contact"] => true,
                _ => false,
            }
        },
    );

    let aex_ns = benchmark_aex_fast_path(&static_routes);
    let best_static = std_ns.min(ahash_ns).min(actix_ns).min(axum_ns).min(aex_ns);

    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "std::HashMap",
        std_ns,
        std_ns / best_static
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "ahash::AHashMap",
        ahash_ns,
        ahash_ns / best_static
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "Actix-web (segment match)",
        actix_ns,
        actix_ns / best_static
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "Axum (segment match)",
        axum_ns,
        axum_ns / best_static
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "AEX Trie (Fast Path)",
        aex_ns,
        aex_ns / best_static
    );
    println!();

    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("                    PARAMETERIZED ROUTE BENCHMARK (ns/op)                      ");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("  {:25} {:>12} {:>12}", "Framework", "ns/op", "vs Best");
    println!("─────────────────────────────────────────────────────────────────────────────");

    let actix_ns = benchmark_framework_param(
        &param_routes.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        |path| {
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            match segs.as_slice() {
                ["api", "users", id] => Some(vec![("id", *id)]),
                ["api", "users", id, "posts"] => Some(vec![("id", *id)]),
                ["api", "posts", slug] => Some(vec![("slug", *slug)]),
                ["api", "posts", slug, "comments", id] => Some(vec![("slug", *slug), ("id", *id)]),
                _ => None,
            }
        },
    );

    let axum_ns = benchmark_framework_param(
        &param_routes.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        |path| {
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            match segs.as_slice() {
                ["api", "users", id] => Some(vec![("id", *id)]),
                ["api", "users", id, "posts"] => Some(vec![("id", *id)]),
                ["api", "posts", slug] => Some(vec![("slug", *slug)]),
                ["api", "posts", slug, "comments", id] => Some(vec![("slug", *slug), ("id", *id)]),
                _ => None,
            }
        },
    );

    let aex_ns = benchmark_aex_param(&param_routes);
    let best_param = actix_ns.min(axum_ns).min(aex_ns);

    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "Actix-web (segment match)",
        actix_ns,
        actix_ns / best_param
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "Axum (segment match)",
        axum_ns,
        axum_ns / best_param
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "AEX Trie",
        aex_ns,
        aex_ns / best_param
    );
    println!(
        "  {:25} {:>10.2} ns  {:>10.2}x",
        "(HashMap N/A for patterns)", 0.0, 0.0
    );
    println!();

    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("                              PERFORMANCE RANKING                              ");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();

    let mut static_results = vec![
        ("std::HashMap", std_ns),
        ("ahash::AHashMap", ahash_ns),
        ("Actix-web", actix_ns),
        ("Axum", axum_ns),
        ("AEX Trie (Fast Path)", aex_ns),
    ];
    static_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    println!("Static Routes:");
    for (i, (name, ns)) in static_results.iter().enumerate() {
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "  ",
        };
        println!("  {}{:25} {:>8.2} ns/op", medal, name, ns);
    }

    let mut param_results = vec![
        ("Actix-web", actix_ns),
        ("Axum", axum_ns),
        ("AEX Trie", aex_ns),
    ];
    param_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    println!();
    println!("Parameterized Routes:");
    for (i, (name, ns)) in param_results.iter().enumerate() {
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "  ",
        };
        println!("  {}{:25} {:>8.2} ns/op", medal, name, ns);
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!("                              KEY INSIGHTS                                   ");
    println!("═══════════════════════════════════════════════════════════════════════════════");
    println!();
    println!("• HashMap is fastest for exact string matching, but CANNOT handle patterns");
    println!("• Actix-web and Axum use Trie-like segment matching internally");
    println!("• AEX Trie handles both static and parameterized routes with single structure");
    println!();
    println!("Real-world comparison:");
    println!("• Pure HashMap: Best for <10 static routes only");
    println!("• AEX/Axum/Actix: Best for real-world APIs with mixed routes");
    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════\n");
}
