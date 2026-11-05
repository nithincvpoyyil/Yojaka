use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use yojaka_core::model::Format;

fn baseline(c: &mut Criterion) {
    let mut g = c.benchmark_group("baseline");
    g.bench_function("format_from_extension", |b| {
        b.iter(|| Format::from_extension(black_box("JPG")))
    });
    let buf = vec![0xA5u8; 1 << 20];
    g.bench_function("blake3_1mb", |b| b.iter(|| blake3::hash(black_box(&buf))));
    g.finish();
}

criterion_group!(benches, baseline);
criterion_main!(benches);
