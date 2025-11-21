//! Benchmark: Chimp128 vs Gorilla encoding/decoding performance
//!
//! This benchmark compares the optimized Chimp128 implementation against Gorilla.
//! Expected results:
//! - Chimp128 encoding: ~300-500 MB/s (similar to Gorilla)
//! - Chimp128 decoding: ~800-1200 MB/s (similar to Gorilla)
//! - Compression: Chimp128 5-15% better than Gorilla
//!
//! Run with: cargo bench --bench chimp128_vs_gorilla

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_encoder, create_decoder};
use std::time::Duration;

/// Generate IoT sensor data (temperature, humidity, pressure, battery)
fn generate_iot_sensor_data(num_points: usize) -> Vec<f64> {
    let mut values = Vec::with_capacity(num_points);
    let mut temp = 23.5;

    for i in 0..num_points {
        // Small fluctuations typical of sensor data
        temp += (i as f64 * 0.01).sin() * 0.1;
        temp = temp.clamp(20.0, 30.0);
        values.push(temp);
    }

    values
}

/// Generate slowly changing float data (typical Gorilla use case)
fn generate_slowly_changing_data(num_points: usize) -> Vec<f32> {
    let mut values = Vec::with_capacity(num_points);
    let mut val = 100.0f32;

    for i in 0..num_points {
        // Very small changes
        val += (i as f32 * 0.001).sin() * 0.01;
        values.push(val);
    }

    values
}

fn bench_encoding_f64(c: &mut Criterion) {
    let mut group = c.benchmark_group("encoding_f64");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_iot_sensor_data(size);
        let bytes = (size * 8) as u64;

        group.throughput(Throughput::Bytes(bytes));

        // Chimp128
        group.bench_with_input(
            BenchmarkId::new("chimp128", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
                    let mut out = Vec::new();
                    for &val in data {
                        encoder.encode_f64(black_box(val), &mut out).unwrap();
                    }
                    encoder.flush(&mut out).unwrap();
                    black_box(out)
                })
            },
        );

        // Gorilla
        group.bench_with_input(
            BenchmarkId::new("gorilla", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
                    let mut out = Vec::new();
                    for &val in data {
                        encoder.encode_f64(black_box(val), &mut out).unwrap();
                    }
                    encoder.flush(&mut out).unwrap();
                    black_box(out)
                })
            },
        );
    }

    group.finish();
}

fn bench_encoding_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("encoding_f32");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_slowly_changing_data(size);
        let bytes = (size * 4) as u64;

        group.throughput(Throughput::Bytes(bytes));

        // Chimp128
        group.bench_with_input(
            BenchmarkId::new("chimp128", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
                    let mut out = Vec::new();
                    for &val in data {
                        encoder.encode_f32(black_box(val), &mut out).unwrap();
                    }
                    encoder.flush(&mut out).unwrap();
                    black_box(out)
                })
            },
        );

        // Gorilla
        group.bench_with_input(
            BenchmarkId::new("gorilla", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
                    let mut out = Vec::new();
                    for &val in data {
                        encoder.encode_f32(black_box(val), &mut out).unwrap();
                    }
                    encoder.flush(&mut out).unwrap();
                    black_box(out)
                })
            },
        );
    }

    group.finish();
}

fn bench_decoding_f64(c: &mut Criterion) {
    let mut group = c.benchmark_group("decoding_f64");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_iot_sensor_data(size);
        let bytes = (size * 8) as u64;

        // Pre-encode with Chimp128
        let mut encoder_c = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
        let mut encoded_chimp = Vec::new();
        for &val in &data {
            encoder_c.encode_f64(val, &mut encoded_chimp).unwrap();
        }
        encoder_c.flush(&mut encoded_chimp).unwrap();

        // Pre-encode with Gorilla
        let mut encoder_g = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut encoded_gorilla = Vec::new();
        for &val in &data {
            encoder_g.encode_f64(val, &mut encoded_gorilla).unwrap();
        }
        encoder_g.flush(&mut encoded_gorilla).unwrap();

        group.throughput(Throughput::Bytes(bytes));

        // Chimp128
        group.bench_with_input(
            BenchmarkId::new("chimp128", size),
            &encoded_chimp,
            |b, encoded| {
                b.iter(|| {
                    let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Double);
                    let mut pos = 0;
                    let mut count = 0;
                    while decoder.has_remaining(encoded, pos) && count < size {
                        let _ = decoder.read_f64(black_box(encoded), &mut pos).unwrap();
                        count += 1;
                    }
                    black_box(count)
                })
            },
        );

        // Gorilla
        group.bench_with_input(
            BenchmarkId::new("gorilla", size),
            &encoded_gorilla,
            |b, encoded| {
                b.iter(|| {
                    let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Double);
                    let mut pos = 0;
                    let mut count = 0;
                    while decoder.has_remaining(encoded, pos) && count < size {
                        let _ = decoder.read_f64(black_box(encoded), &mut pos).unwrap();
                        count += 1;
                    }
                    black_box(count)
                })
            },
        );
    }

    group.finish();
}

fn bench_compression_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_ratio");
    group.measurement_time(Duration::from_secs(5));

    let size = 100_000;
    let data = generate_iot_sensor_data(size);

    println!("\n=== Compression Ratio Comparison ===");
    println!("Dataset: {} f64 values = {} KB raw", size, size * 8 / 1024);

    // Chimp128
    let mut encoder_c = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut encoded_chimp = Vec::new();
    for &val in &data {
        encoder_c.encode_f64(val, &mut encoded_chimp).unwrap();
    }
    encoder_c.flush(&mut encoded_chimp).unwrap();

    // Gorilla
    let mut encoder_g = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut encoded_gorilla = Vec::new();
    for &val in &data {
        encoder_g.encode_f64(val, &mut encoded_gorilla).unwrap();
    }
    encoder_g.flush(&mut encoded_gorilla).unwrap();

    let raw_size = size * 8;
    let chimp_ratio = raw_size as f64 / encoded_chimp.len() as f64;
    let gorilla_ratio = raw_size as f64 / encoded_gorilla.len() as f64;
    let improvement = ((encoded_gorilla.len() as f64 - encoded_chimp.len() as f64) / encoded_gorilla.len() as f64) * 100.0;

    println!("Chimp128: {} bytes = {:.2}:1 compression", encoded_chimp.len(), chimp_ratio);
    println!("Gorilla:  {} bytes = {:.2}:1 compression", encoded_gorilla.len(), gorilla_ratio);
    println!("Chimp128 improvement: {:.1}% smaller than Gorilla\n", improvement);

    group.finish();
}

criterion_group!(
    benches,
    bench_encoding_f64,
    bench_encoding_f32,
    bench_decoding_f64,
    bench_compression_ratio
);
criterion_main!(benches);
