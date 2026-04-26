use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::net::SocketAddr;
use std::sync::Arc;

fn bench_connection_manager_creation(c: &mut Criterion) {
    use aex::connection::manager::ConnectionManager;

    c.bench_function("connection_manager_new", |b| {
        b.iter(|| {
            let manager = ConnectionManager::new();
            black_box(manager);
        });
    });
}

fn bench_connection_manager_lookup(c: &mut Criterion) {
    use aex::connection::manager::ConnectionManager;
    use aex::connection::scope::NetworkScope;

    let manager = ConnectionManager::new();
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let scope = NetworkScope::from_ip(&addr.ip());

    c.bench_function("connection_manager_lookup", |b| {
        b.iter(|| {
            let _ = black_box(manager.connections.get(&(addr.ip(), scope)));
        });
    });
}

fn bench_global_context_creation(c: &mut Criterion) {
    use aex::connection::global::GlobalContext;

    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    c.bench_function("global_context_new", |b| {
        b.iter(|| {
            let global = GlobalContext::new(addr, None);
            black_box(global);
        });
    });
}

fn bench_network_scope_from_ip(c: &mut Criterion) {
    use aex::connection::scope::NetworkScope;

    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    c.bench_function("network_scope_from_ip", |b| {
        b.iter(|| {
            let scope = NetworkScope::from_ip(&addr.ip());
            black_box(scope);
        });
    });
}

fn bench_protocol_from_str(c: &mut Criterion) {
    use aex::connection::protocol::Protocol;

    c.bench_function("protocol_from_str", |b| {
        b.iter(|| {
            let protocol = Protocol::from("http");
            black_box(protocol);
        });
    });
}

criterion_group!(
    benches,
    bench_connection_manager_creation,
    bench_connection_manager_lookup,
    bench_global_context_creation,
    bench_network_scope_from_ip,
    bench_protocol_from_str
);
criterion_main!(benches);
