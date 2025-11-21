//! Benchmark: DictionaryRLE raw encoding performance
//!
//! Tests encoding and decoding speed for DictionaryRLE with different data patterns:
//! 1. High repetition (best case)
//! 2. Medium repetition (typical IoT)
//! 3. Low repetition (worst case)
//!
//! Run with: cargo bench --bench dictionary_rle_perf

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Duration;
use timbre_tsf::encoding::dictionary_rle::DictionaryRLEEncoder;

/// Generate high repetition data (best case: 95% repetition, 4 unique values)
fn generate_high_repetition(n: usize) -> Vec<f64> {
    let values = vec![20.0, 20.1, 20.2, 20.3];
    let mut data = Vec::with_capacity(n);
    let mut idx = 0;

    for i in 0..n {
        data.push(values[idx]);
        // Change value every ~50 points (95% repetition)
        if i % 50 == 0 && i > 0 {
            idx = (idx + 1) % values.len();
        }
    }

    data
}

/// Generate medium repetition data (typical IoT: 85% repetition, 20 unique values)
fn generate_medium_repetition(n: usize) -> Vec<f64> {
    let values: Vec<f64> = (0..20).map(|i| 20.0 + i as f64 * 0.1).collect();
    let mut data = Vec::with_capacity(n);
    let mut idx = 0;

    for i in 0..n {
        data.push(values[idx]);
        // Change value every ~15 points (85% repetition)
        if i % 15 == 0 && i > 0 {
            idx = (idx + 1) % values.len();
        }
    }

    data
}

/// Generate low repetition data (worst case: 50% repetition, 100 unique values)
fn generate_low_repetition(n: usize) -> Vec<f64> {
    let values: Vec<f64> = (0..100).map(|i| 20.0 + i as f64 * 0.05).collect();
    let mut data = Vec::with_capacity(n);
    let mut idx = 0;

    for i in 0..n {
        data.push(values[idx]);
        // Change value every ~5 points (50% repetition)
        if i % 5 == 0 && i > 0 {
            idx = (idx + 1) % values.len();
        }
    }

    data
}

/// Benchmark encoding with high repetition
fn bench_encode_high_repetition(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_rle_encode_high_rep");
    group.measurement_time(Duration::from_secs(5));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_high_repetition(size);
        group.throughput(Throughput::Bytes((size * 8) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| {
                let mut encoder = DictionaryRLEEncoder::new();
                let encoded = encoder.encode(black_box(data)).unwrap();
                black_box(encoded)
            });
        });
    }

    group.finish();
}

/// Benchmark encoding with medium repetition
fn bench_encode_medium_repetition(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_rle_encode_med_rep");
    group.measurement_time(Duration::from_secs(5));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_medium_repetition(size);
        group.throughput(Throughput::Bytes((size * 8) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| {
                let mut encoder = DictionaryRLEEncoder::new();
                let encoded = encoder.encode(black_box(data)).unwrap();
                black_box(encoded)
            });
        });
    }

    group.finish();
}

/// Benchmark encoding with low repetition
fn bench_encode_low_repetition(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_rle_encode_low_rep");
    group.measurement_time(Duration::from_secs(5));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_low_repetition(size);
        group.throughput(Throughput::Bytes((size * 8) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| {
                let mut encoder = DictionaryRLEEncoder::new();
                let encoded = encoder.encode(black_box(data)).unwrap();
                black_box(encoded)
            });
        });
    }

    group.finish();
}

/// Benchmark decoding with high repetition
fn bench_decode_high_repetition(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_rle_decode_high_rep");
    group.measurement_time(Duration::from_secs(5));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_high_repetition(size);
        let mut encoder = DictionaryRLEEncoder::new();
        let encoded = encoder.encode(&data).unwrap();

        group.throughput(Throughput::Bytes((size * 8) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let encoder = DictionaryRLEEncoder::new();
                let decoded = encoder.decode(black_box(encoded)).unwrap();
                black_box(decoded)
            });
        });
    }

    group.finish();
}

/// Benchmark decoding with medium repetition
fn bench_decode_medium_repetition(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_rle_decode_med_rep");
    group.measurement_time(Duration::from_secs(5));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_medium_repetition(size);
        let mut encoder = DictionaryRLEEncoder::new();
        let encoded = encoder.encode(&data).unwrap();

        group.throughput(Throughput::Bytes((size * 8) as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let encoder = DictionaryRLEEncoder::new();
                let decoded = encoder.decode(black_box(encoded)).unwrap();
                black_box(decoded)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_encode_high_repetition,
    bench_encode_medium_repetition,
    bench_encode_low_repetition,
    bench_decode_high_repetition,
    bench_decode_medium_repetition,
);
criterion_main!(benches);
