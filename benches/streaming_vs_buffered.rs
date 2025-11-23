// Benchmark: Streaming decode vs Buffered decode
//
// Tests if streaming decode (without Vec) is faster than buffered decode

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timbre_tsf::encoding::gorilla::{GorillaEncoder, GorillaDecoder};
use timbre_tsf::encoding::{Encoder, Decoder};
use timbre_tsf::common::types::TSDataType;

fn generate_f32_data(count: usize) -> Vec<f32> {
    (0..count).map(|i| (i as f32) * 0.1).collect()
}

fn encode_data(values: &[f32]) -> Vec<u8> {
    let mut encoder = GorillaEncoder::new(TSDataType::Float);
    let mut out = Vec::new();
    for &val in values {
        encoder.encode_f32(val, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();
    out
}

// Current approach: decode to Vec<f32>
fn decode_to_vec(data: &[u8], count: usize) -> Vec<f32> {
    let mut decoder = GorillaDecoder::new(TSDataType::Float);
    let mut pos = 0;
    let mut output = Vec::with_capacity(count);

    for _ in 0..count {
        if let Ok(val) = decoder.read_f32(data, &mut pos) {
            output.push(val);
        }
    }
    output
}

// Proposed: streaming decode with aggregation (no Vec)
fn decode_streaming_sum(data: &[u8], count: usize) -> f64 {
    let mut decoder = GorillaDecoder::new(TSDataType::Float);
    let mut pos = 0;
    let mut sum = 0.0f64;

    for _ in 0..count {
        if let Ok(val) = decoder.read_f32(data, &mut pos) {
            sum += val as f64;
        }
    }
    sum
}

// Proposed: streaming decode with filter (partial Vec)
fn decode_streaming_filter(data: &[u8], count: usize, threshold: f32) -> Vec<f32> {
    let mut decoder = GorillaDecoder::new(TSDataType::Float);
    let mut pos = 0;
    let mut output = Vec::new();

    for _ in 0..count {
        if let Ok(val) = decoder.read_f32(data, &mut pos) {
            if val > threshold {
                output.push(val);
            }
        }
    }
    output
}

// Baseline: decode to Vec then sum (current approach)
fn decode_then_sum(data: &[u8], count: usize) -> f64 {
    let values = decode_to_vec(data, count);
    values.iter().map(|&v| v as f64).sum()
}

// Baseline: decode to Vec then filter (current approach)
fn decode_then_filter(data: &[u8], count: usize, threshold: f32) -> Vec<f32> {
    let values = decode_to_vec(data, count);
    values.into_iter().filter(|&v| v > threshold).collect()
}

fn benchmark_decode_approaches(c: &mut Criterion) {
    let sizes = [1000, 10000, 100000];

    for &size in &sizes {
        let values = generate_f32_data(size);
        let encoded = encode_data(&values);

        let mut group = c.benchmark_group(format!("decode_{}_values", size));

        // 1. Decode to Vec (current)
        group.bench_with_input(
            BenchmarkId::new("buffered_decode", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let vec = decode_to_vec(black_box(data), black_box(size));
                    black_box(vec);
                })
            },
        );

        // 2. Sum: Decode then sum (current)
        group.bench_with_input(
            BenchmarkId::new("buffered_then_sum", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let sum = decode_then_sum(black_box(data), black_box(size));
                    black_box(sum);
                })
            },
        );

        // 3. Sum: Streaming decode (proposed)
        group.bench_with_input(
            BenchmarkId::new("streaming_sum", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let sum = decode_streaming_sum(black_box(data), black_box(size));
                    black_box(sum);
                })
            },
        );

        // 4. Filter (10% matches): Decode then filter (current)
        let threshold = size as f32 * 0.09; // ~90% of values match
        group.bench_with_input(
            BenchmarkId::new("buffered_then_filter_10pct", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let filtered = decode_then_filter(black_box(data), black_box(size), black_box(threshold));
                    black_box(filtered);
                })
            },
        );

        // 5. Filter (10% matches): Streaming filter (proposed)
        group.bench_with_input(
            BenchmarkId::new("streaming_filter_10pct", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let filtered = decode_streaming_filter(black_box(data), black_box(size), black_box(threshold));
                    black_box(filtered);
                })
            },
        );

        group.finish();
    }
}

fn benchmark_memory_overhead(c: &mut Criterion) {
    // Test if Vec::push overhead is significant
    let size = 100000;
    let values = generate_f32_data(size);
    let encoded = encode_data(&values);

    let mut group = c.benchmark_group("memory_operations");

    // Just decode (no storage)
    group.bench_with_input(
        BenchmarkId::new("decode_only", size),
        &encoded,
        |b, data| {
            b.iter(|| {
                let mut decoder = GorillaDecoder::new(TSDataType::Float);
                let mut pos = 0;
                let mut sink = 0.0f32;

                for _ in 0..size {
                    if let Ok(val) = decoder.read_f32(black_box(data), &mut pos) {
                        sink = val; // Just to prevent optimization
                    }
                }
                black_box(sink);
            })
        },
    );

    // Decode + Vec::push (with capacity)
    group.bench_with_input(
        BenchmarkId::new("decode_push_with_capacity", size),
        &encoded,
        |b, data| {
            b.iter(|| {
                let mut decoder = GorillaDecoder::new(TSDataType::Float);
                let mut pos = 0;
                let mut output = Vec::with_capacity(size);

                for _ in 0..size {
                    if let Ok(val) = decoder.read_f32(black_box(data), &mut pos) {
                        output.push(val);
                    }
                }
                black_box(output);
            })
        },
    );

    // Decode + Vec::push (without capacity - triggers reallocs)
    group.bench_with_input(
        BenchmarkId::new("decode_push_no_capacity", size),
        &encoded,
        |b, data| {
            b.iter(|| {
                let mut decoder = GorillaDecoder::new(TSDataType::Float);
                let mut pos = 0;
                let mut output = Vec::new();

                for _ in 0..size {
                    if let Ok(val) = decoder.read_f32(black_box(data), &mut pos) {
                        output.push(val);
                    }
                }
                black_box(output);
            })
        },
    );

    group.finish();
}

criterion_group!(benches, benchmark_decode_approaches, benchmark_memory_overhead);
criterion_main!(benches);
