use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_json_encode(c: &mut Criterion) {
    #[derive(serde::Serialize)]
    struct TestData {
        id: u64,
        name: String,
    }

    let data = TestData {
        id: 12345,
        name: "test".to_string(),
    };

    c.bench_function("json_encode", |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(&data).unwrap();
            black_box(encoded);
        });
    });
}

fn bench_json_decode(c: &mut Criterion) {
    #[derive(serde::Deserialize, serde::Serialize)]
    struct TestData {
        id: u64,
        name: String,
    }

    let data = TestData {
        id: 12345,
        name: "test".to_string(),
    };
    let encoded = serde_json::to_string(&data).unwrap();

    c.bench_function("json_decode", |b| {
        b.iter(|| {
            let decoded: TestData = serde_json::from_str(&encoded).unwrap();
            black_box(decoded);
        });
    });
}

fn bench_base64_encode(c: &mut Criterion) {
    let data = vec![1u8; 1024];

    c.bench_function("base64_encode_1kb", |b| {
        b.iter(|| {
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
            black_box(encoded);
        });
    });
}

fn bench_base64_decode(c: &mut Criterion) {
    let data = vec![1u8; 1024];
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    c.bench_function("base64_decode_1kb", |b| {
        b.iter(|| {
            let decoded =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &encoded)
                    .unwrap();
            black_box(decoded);
        });
    });
}

fn bench_protocol_as_str(c: &mut Criterion) {
    use aex::connection::protocol::Protocol;

    c.bench_function("protocol_as_str", |b| {
        b.iter(|| {
            let protocol = Protocol::Ws;
            let s = protocol.as_str();
            black_box(s);
        });
    });
}

criterion_group!(
    benches,
    bench_json_encode,
    bench_json_decode,
    bench_base64_encode,
    bench_base64_decode,
    bench_protocol_as_str
);
criterion_main!(benches);
