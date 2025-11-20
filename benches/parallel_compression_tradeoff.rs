//! Benchmark: Parallel Compression Trade-off
//!
//! Compara diferentes estrategias de compresión para miniblocks:
//!
//! 1. **Zstd Serial (monolítico):**
//!    - Mejor compression ratio
//!    - Throughput limitado (serial)
//!
//! 2. **LZ4 Paralelo (miniblocks independientes):**
//!    - Compression ratio menor
//!    - Throughput alto (8x speedup potencial)
//!
//! 3. **Zstd Paralelo (miniblocks sin dictionary):**
//!    - Compression ratio medio
//!    - Throughput alto
//!
//! Run with: cargo bench --bench parallel_compression_tradeoff

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timbre_tsf::common::*;
use timbre_tsf::compress::create_compressor;
use timbre_tsf::encoding::create_encoder;
use std::time::Duration;

/// Genera datos encoded realistas (Gorilla + timestamps)
fn generate_encoded_data(num_points: usize) -> (Vec<u8>, Vec<u8>) {
    // Timestamps
    let mut time_encoder = create_encoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
    let mut time_buffer = Vec::new();

    for i in 0..num_points {
        let ts = 1_000_000_000 + (i as i64 * 1000); // 1 sample/s
        time_encoder.encode_i64(ts, &mut time_buffer).unwrap();
    }
    time_encoder.flush(&mut time_buffer).unwrap();

    // Values (sensor drift realista)
    let mut value_encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut value_buffer = Vec::new();

    for i in 0..num_points {
        let value = 20.0 + (i as f64 * 0.001).sin() * 5.0; // Slow drift
        value_encoder.encode_f64(value, &mut value_buffer).unwrap();
    }
    value_encoder.flush(&mut value_buffer).unwrap();

    (time_buffer, value_buffer)
}

/// Compresión serial (Zstd monolítico)
fn compress_serial_zstd(time_data: &[u8], value_data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut compressor = create_compressor(CompressionType::Zstd);

    let time_compressed = compressor.compress(time_data).unwrap();
    let value_compressed = compressor.compress(value_data).unwrap();

    (time_compressed, value_compressed)
}

/// Compresión paralela con LZ4 (miniblocks independientes)
fn compress_parallel_lz4(time_data: &[u8], value_data: &[u8], num_miniblocks: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
    use rayon::prelude::*;

    let chunk_size = time_data.len() / num_miniblocks;
    let time_chunks: Vec<&[u8]> = time_data.chunks(chunk_size).collect();
    let value_chunks: Vec<&[u8]> = value_data.chunks(chunk_size).collect();

    time_chunks.par_iter().zip(value_chunks.par_iter())
        .map(|(time_chunk, value_chunk)| {
            let mut compressor = create_compressor(CompressionType::Lz4);
            let time_compressed = compressor.compress(time_chunk).unwrap();
            let value_compressed = compressor.compress(value_chunk).unwrap();
            (time_compressed, value_compressed)
        })
        .collect()
}

/// Compresión paralela con Zstd (miniblocks sin dictionary)
fn compress_parallel_zstd(time_data: &[u8], value_data: &[u8], num_miniblocks: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
    use rayon::prelude::*;

    let chunk_size = time_data.len() / num_miniblocks;
    let time_chunks: Vec<&[u8]> = time_data.chunks(chunk_size).collect();
    let value_chunks: Vec<&[u8]> = value_data.chunks(chunk_size).collect();

    time_chunks.par_iter().zip(value_chunks.par_iter())
        .map(|(time_chunk, value_chunk)| {
            let mut compressor = create_compressor(CompressionType::Zstd);
            let time_compressed = compressor.compress(time_chunk).unwrap();
            let value_compressed = compressor.compress(value_chunk).unwrap();
            (time_compressed, value_compressed)
        })
        .collect()
}

/// Benchmark: Serial Zstd
fn bench_serial_zstd(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_serial_zstd");
    group.measurement_time(Duration::from_secs(15));

    for size in [10_000, 50_000, 100_000] {
        let (time_data, value_data) = generate_encoded_data(size);
        let raw_size = time_data.len() + value_data.len();

        // Calculate compression ratio first
        let (time_c, value_c) = compress_serial_zstd(&time_data, &value_data);
        let compressed_size = time_c.len() + value_c.len();
        let ratio = raw_size as f64 / compressed_size as f64;
        println!("Serial Zstd {}pts: {}B → {}B ({:.2}x)", size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}pts", size)),
            &(time_data, value_data),
            |b, (time, value)| {
                b.iter(|| {
                    let result = compress_serial_zstd(black_box(time), black_box(value));
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Parallel LZ4
fn bench_parallel_lz4(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_parallel_lz4");
    group.measurement_time(Duration::from_secs(15));

    for size in [10_000, 50_000, 100_000] {
        let (time_data, value_data) = generate_encoded_data(size);
        let raw_size = time_data.len() + value_data.len();

        for num_mb in [4, 8] {
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{}pts_{}mb", size, num_mb)),
                &(time_data.clone(), value_data.clone(), num_mb),
                |b, (time, value, mb)| {
                    b.iter(|| {
                        let result = compress_parallel_lz4(black_box(time), black_box(value), *mb);
                        black_box(result)
                    });
                },
            );

            // Print compression ratio
            let compressed_blocks = compress_parallel_lz4(&time_data, &value_data, num_mb);
            let compressed_size: usize = compressed_blocks.iter()
                .map(|(t, v)| t.len() + v.len())
                .sum();
            let ratio = raw_size as f64 / compressed_size as f64;
            println!("Parallel LZ4 {}pts {}mb: {}B → {}B ({:.2}x)",
                     size, num_mb, raw_size, compressed_size, ratio);
        }
    }

    group.finish();
}

/// Benchmark: Parallel Zstd (sin dictionary)
fn bench_parallel_zstd(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_parallel_zstd");
    group.measurement_time(Duration::from_secs(15));

    for size in [10_000, 50_000, 100_000] {
        let (time_data, value_data) = generate_encoded_data(size);
        let raw_size = time_data.len() + value_data.len();

        for num_mb in [4, 8] {
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{}pts_{}mb", size, num_mb)),
                &(time_data.clone(), value_data.clone(), num_mb),
                |b, (time, value, mb)| {
                    b.iter(|| {
                        let result = compress_parallel_zstd(black_box(time), black_box(value), *mb);
                        black_box(result)
                    });
                },
            );

            // Print compression ratio
            let compressed_blocks = compress_parallel_zstd(&time_data, &value_data, num_mb);
            let compressed_size: usize = compressed_blocks.iter()
                .map(|(t, v)| t.len() + v.len())
                .sum();
            let ratio = raw_size as f64 / compressed_size as f64;
            println!("Parallel Zstd {}pts {}mb: {}B → {}B ({:.2}x)",
                     size, num_mb, raw_size, compressed_size, ratio);
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_serial_zstd,
    bench_parallel_lz4,
    bench_parallel_zstd,
);
criterion_main!(benches);
