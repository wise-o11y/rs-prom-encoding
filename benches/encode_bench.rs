use criterion::{criterion_group, criterion_main, Criterion};
use rs_prom_encoder::XORChunk;

fn bench_xor_encode(c: &mut Criterion) {
    // Generate 120 samples (typical Prometheus scrape chunk)
    let samples: Vec<(i64, f64)> = (0..120)
        .map(|i| {
            let t = 1_700_000_000_000i64 + i * 15_000; // 15s intervals
            let v = 100.0 + (i as f64) * 0.1;
            (t, v)
        })
        .collect();

    c.bench_function("xor_encode_120_samples", |b| {
        b.iter(|| {
            let mut chunk = XORChunk::new();
            for &(t, v) in &samples {
                chunk.append(t, v);
            }
            chunk.encode()
        })
    });
}

criterion_group!(benches, bench_xor_encode);
criterion_main!(benches);
