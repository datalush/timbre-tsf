//! Benchmark: Batch vs Individual Decoding Performance
//!
//! This benchmark compares batch decoding against individual value decoding
//! for Chimp128 encoding. Expected improvement: 25-35% faster with batch decoding.
//!
//! Run with: cargo bench --bench batch_decoding

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Duration;
use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_decoder, create_encoder};

/// Generate IoT sensor data (temperature with small fluctuations)
fn generate_sensor_data_f64(num_points: usize) -> Vec<f64> {
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

/// Generate sensor data for f32
fn generate_sensor_data_f32(num_points: usize) -> Vec<f32> {
    let mut values = Vec::with_capacity(num_points);
    let mut val = 100.0f32;

    for i in 0..num_points {
        // Small changes
        val += (i as f32 * 0.001).sin() * 0.01;
        values.push(val);
    }

    values
}

/// Encode data once and return encoded bytes
fn encode_f64_data(data: &[f64]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut out = Vec::new();
    for &val in data {
        encoder.encode_f64(val, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();
    out
}

/// Encode data once and return encoded bytes
fn encode_f32_data(data: &[f32]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::new();
    for &val in data {
        encoder.encode_f32(val, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();
    out
}

fn bench_decoding_f64_individual(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_decode_f64_individual");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_sensor_data_f64(size);
        let encoded = encode_f64_data(&data);
        let bytes = (size * 8) as u64;

        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Double);
                let mut pos = 0;
                let mut results = Vec::with_capacity(size);

                for _ in 0..size {
                    let val = decoder.read_f64(black_box(encoded), &mut pos).unwrap();
                    results.push(val);
                }

                black_box(results)
            })
        });
    }

    group.finish();
}

fn bench_decoding_f64_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_decode_f64_batch");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_sensor_data_f64(size);
        let encoded = encode_f64_data(&data);
        let bytes = (size * 8) as u64;

        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Double);
                let mut pos = 0;
                let mut results = Vec::new();

                decoder.read_f64_batch(black_box(encoded), &mut pos, &mut results, size).unwrap();

                black_box(results)
            })
        });
    }

    group.finish();
}

fn bench_decoding_f32_individual(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_decode_f32_individual");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_sensor_data_f32(size);
        let encoded = encode_f32_data(&data);
        let bytes = (size * 4) as u64;

        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Float);
                let mut pos = 0;
                let mut results = Vec::with_capacity(size);

                for _ in 0..size {
                    let val = decoder.read_f32(black_box(encoded), &mut pos).unwrap();
                    results.push(val);
                }

                black_box(results)
            })
        });
    }

    group.finish();
}

fn bench_decoding_f32_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_decode_f32_batch");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 100_000, 1_000_000] {
        let data = generate_sensor_data_f32(size);
        let encoded = encode_f32_data(&data);
        let bytes = (size * 4) as u64;

        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Float);
                let mut pos = 0;
                let mut results = Vec::new();

                decoder.read_f32_batch(black_box(encoded), &mut pos, &mut results, size).unwrap();

                black_box(results)
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_decoding_f64_individual,
    bench_decoding_f64_batch,
    bench_decoding_f32_individual,
    bench_decoding_f32_batch
);
criterion_main!(benches);
