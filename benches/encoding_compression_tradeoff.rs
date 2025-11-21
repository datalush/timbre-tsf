//! Benchmark: Encoding-Specific Compression Trade-off
//!
//! Mide el trade-off REAL para los encodings de Timbre:
//!
//! **Caso 1: Quantized + Simple8b** (datos discretos IoT)
//! - Serial Zstd: mejor ratio, lento
//! - Paralelo LZ4: peor ratio, 8x speedup
//!
//! **Caso 2: Chimp128** (datos continuos)
//! - Serial Zstd: mejor ratio, lento
//! - Paralelo LZ4: peor ratio, 8x speedup
//!
//! Run with: cargo bench --bench encoding_compression_tradeoff

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timbre_tsf::common::*;
use timbre_tsf::compress::create_compressor;
use timbre_tsf::encoding::quantized::{QuantizedEncoder, detect_quantization};
use timbre_tsf::encoding::dictionary_rle::DictionaryRLEEncoder;
use timbre_tsf::encoding::chimp128::Chimp128Encoder;
use timbre_tsf::encoding::create_encoder;
use std::time::Duration;

/// Genera datos cuantizados realistas (sensor IoT con 0.1°C precisión)
fn generate_quantized_data(num_points: usize) -> (Vec<i64>, Vec<f64>) {
    let mut timestamps = Vec::with_capacity(num_points);
    let mut values = Vec::with_capacity(num_points);

    let mut temp = 20.0_f64;
    for i in 0..num_points {
        timestamps.push(1_000_000_000 + (i as i64 * 1000));
        values.push(temp);

        // Cambios ocasionales de 0.1°C
        if i % 10 == 0 {
            temp += if i % 2 == 0 { 0.1 } else { -0.1 };
            temp = temp.clamp(19.0, 21.0);
        }
    }

    (timestamps, values)
}

/// Genera datos continuos realistas (sensor analógico)
fn generate_continuous_data(num_points: usize) -> (Vec<i64>, Vec<f64>) {
    let mut timestamps = Vec::with_capacity(num_points);
    let mut values = Vec::with_capacity(num_points);

    for i in 0..num_points {
        timestamps.push(1_000_000_000 + (i as i64 * 1000));
        let value = 20.0 + (i as f64 * 0.001).sin() * 5.0;
        values.push(value);
    }

    (timestamps, values)
}

/// Genera datos con alta repetición (sensor IoT estable)
fn generate_high_repetition_data(num_points: usize) -> (Vec<i64>, Vec<f64>) {
    let mut timestamps = Vec::with_capacity(num_points);
    let mut values = Vec::with_capacity(num_points);

    let value_set = vec![20.0, 20.1, 20.3, 20.7]; // 4 valores únicos
    let mut current_value_idx = 0;

    for i in 0..num_points {
        timestamps.push(1_000_000_000 + (i as i64 * 1000));
        values.push(value_set[current_value_idx]);

        // Cambiar valor cada ~25 puntos (85% repetición promedio)
        if i % 25 == 0 && i > 0 {
            current_value_idx = (current_value_idx + 1) % value_set.len();
        }
    }

    (timestamps, values)
}

/// Encodea timestamps con DeltaOfDelta
fn encode_timestamps(timestamps: &[i64]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
    let mut buffer = Vec::new();

    for &ts in timestamps {
        encoder.encode_i64(ts, &mut buffer).unwrap();
    }
    encoder.flush(&mut buffer).unwrap();

    buffer
}

/// Caso 1A: Quantized + Simple8b → Zstd serial
fn bench_quantized_zstd_serial(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantized_zstd_serial");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_quantized_data(size);

        // Encode
        let time_encoded = encode_timestamps(&timestamps);

        let (min, step) = detect_quantization(&values).unwrap();
        let mut encoder = QuantizedEncoder::new(min, step);
        let value_encoded = encoder.encode(&values).unwrap();

        let raw_size = time_encoded.len() + value_encoded.len();

        // Compress serial
        let mut compressor = create_compressor(CompressionType::Zstd);
        let time_compressed = compressor.compress(&time_encoded).unwrap();
        let value_compressed = compressor.compress(&value_encoded).unwrap();
        let compressed_size = time_compressed.len() + value_compressed.len();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("Quantized+Zstd Serial {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(time_encoded, value_encoded),
            |b, (time, value)| {
                b.iter(|| {
                    let mut compressor = create_compressor(CompressionType::Zstd);
                    let tc = compressor.compress(black_box(time)).unwrap();
                    let vc = compressor.compress(black_box(value)).unwrap();
                    black_box((tc, vc))
                });
            },
        );
    }

    group.finish();
}

/// Caso 1B: Quantized + Simple8b → LZ4 paralelo (8 miniblocks)
fn bench_quantized_lz4_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantized_lz4_parallel");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_quantized_data(size);

        // Dividir en 8 miniblocks y encodear cada uno
        let chunk_size = size / 8;
        let mut encoded_blocks = Vec::new();

        for i in 0..8 {
            let start = i * chunk_size;
            let end = if i == 7 { size } else { (i + 1) * chunk_size };

            let time_chunk = &timestamps[start..end];
            let value_chunk = &values[start..end];

            let time_enc = encode_timestamps(time_chunk);

            let (min, step) = detect_quantization(value_chunk).unwrap();
            let mut encoder = QuantizedEncoder::new(min, step);
            let value_enc = encoder.encode(value_chunk).unwrap();

            encoded_blocks.push((time_enc, value_enc));
        }

        let raw_size: usize = encoded_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();

        // Compress parallel
        use rayon::prelude::*;
        let compressed_blocks: Vec<(Vec<u8>, Vec<u8>)> = encoded_blocks
            .par_iter()
            .map(|(time, value)| {
                let mut compressor = create_compressor(CompressionType::Lz4);
                let tc = compressor.compress(time).unwrap();
                let vc = compressor.compress(value).unwrap();
                (tc, vc)
            })
            .collect();

        let compressed_size: usize = compressed_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("Quantized+LZ4 Parallel {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &encoded_blocks,
            |b, blocks| {
                b.iter(|| {
                    use rayon::prelude::*;
                    let result: Vec<(Vec<u8>, Vec<u8>)> = blocks
                        .par_iter()
                        .map(|(time, value)| {
                            let mut compressor = create_compressor(CompressionType::Lz4);
                            let tc = compressor.compress(black_box(time)).unwrap();
                            let vc = compressor.compress(black_box(value)).unwrap();
                            (tc, vc)
                        })
                        .collect();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Caso 2A: Chimp128 → Zstd serial
fn bench_chimp128_zstd_serial(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_zstd_serial");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_continuous_data(size);

        // Encode
        let time_encoded = encode_timestamps(&timestamps);

        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
        let mut value_encoded = Vec::new();
        for &val in &values {
            encoder.encode_f64(val, &mut value_encoded).unwrap();
        }
        encoder.flush(&mut value_encoded).unwrap();

        let raw_size = time_encoded.len() + value_encoded.len();

        // Compress serial
        let mut compressor = create_compressor(CompressionType::Zstd);
        let time_compressed = compressor.compress(&time_encoded).unwrap();
        let value_compressed = compressor.compress(&value_encoded).unwrap();
        let compressed_size = time_compressed.len() + value_compressed.len();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("Chimp128+Zstd Serial {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(time_encoded, value_encoded),
            |b, (time, value)| {
                b.iter(|| {
                    let mut compressor = create_compressor(CompressionType::Zstd);
                    let tc = compressor.compress(black_box(time)).unwrap();
                    let vc = compressor.compress(black_box(value)).unwrap();
                    black_box((tc, vc))
                });
            },
        );
    }

    group.finish();
}

/// Caso 2B: Chimp128 → LZ4 paralelo (8 miniblocks)
fn bench_chimp128_lz4_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("chimp128_lz4_parallel");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_continuous_data(size);

        // Dividir en 8 miniblocks y encodear cada uno
        let chunk_size = size / 8;
        let mut encoded_blocks = Vec::new();

        for i in 0..8 {
            let start = i * chunk_size;
            let end = if i == 7 { size } else { (i + 1) * chunk_size };

            let time_chunk = &timestamps[start..end];
            let value_chunk = &values[start..end];

            let time_enc = encode_timestamps(time_chunk);

            let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
            let mut value_enc = Vec::new();
            for &val in value_chunk {
                encoder.encode_f64(val, &mut value_enc).unwrap();
            }
            encoder.flush(&mut value_enc).unwrap();

            encoded_blocks.push((time_enc, value_enc));
        }

        let raw_size: usize = encoded_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();

        // Compress parallel
        use rayon::prelude::*;
        let compressed_blocks: Vec<(Vec<u8>, Vec<u8>)> = encoded_blocks
            .par_iter()
            .map(|(time, value)| {
                let mut compressor = create_compressor(CompressionType::Lz4);
                let tc = compressor.compress(time).unwrap();
                let vc = compressor.compress(value).unwrap();
                (tc, vc)
            })
            .collect();

        let compressed_size: usize = compressed_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("Chimp128+LZ4 Parallel {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &encoded_blocks,
            |b, blocks| {
                b.iter(|| {
                    use rayon::prelude::*;
                    let result: Vec<(Vec<u8>, Vec<u8>)> = blocks
                        .par_iter()
                        .map(|(time, value)| {
                            let mut compressor = create_compressor(CompressionType::Lz4);
                            let tc = compressor.compress(black_box(time)).unwrap();
                            let vc = compressor.compress(black_box(value)).unwrap();
                            (tc, vc)
                        })
                        .collect();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Caso 3A: DictionaryRLE → Zstd serial
fn bench_dictionary_zstd_serial(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_zstd_serial");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_high_repetition_data(size);

        // Encode
        let time_encoded = encode_timestamps(&timestamps);

        let mut encoder = DictionaryRLEEncoder::new();
        let value_encoded = encoder.encode(&values).unwrap();

        let raw_size = time_encoded.len() + value_encoded.len();

        // Compress serial
        let mut compressor = create_compressor(CompressionType::Zstd);
        let time_compressed = compressor.compress(&time_encoded).unwrap();
        let value_compressed = compressor.compress(&value_encoded).unwrap();
        let compressed_size = time_compressed.len() + value_compressed.len();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("DictionaryRLE+Zstd Serial {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(time_encoded, value_encoded),
            |b, (time, value)| {
                b.iter(|| {
                    let mut compressor = create_compressor(CompressionType::Zstd);
                    let tc = compressor.compress(black_box(time)).unwrap();
                    let vc = compressor.compress(black_box(value)).unwrap();
                    black_box((tc, vc))
                });
            },
        );
    }

    group.finish();
}

/// Caso 3B: DictionaryRLE → LZ4 paralelo (8 miniblocks)
fn bench_dictionary_lz4_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("dictionary_lz4_parallel");
    group.measurement_time(Duration::from_secs(10));

    for size in [10_000, 50_000, 100_000] {
        let (timestamps, values) = generate_high_repetition_data(size);

        // Dividir en 8 miniblocks y encodear cada uno
        let chunk_size = size / 8;
        let mut encoded_blocks = Vec::new();

        for i in 0..8 {
            let start = i * chunk_size;
            let end = if i == 7 { size } else { (i + 1) * chunk_size };

            let time_chunk = &timestamps[start..end];
            let value_chunk = &values[start..end];

            let time_enc = encode_timestamps(time_chunk);

            let mut encoder = DictionaryRLEEncoder::new();
            let value_enc = encoder.encode(value_chunk).unwrap();

            encoded_blocks.push((time_enc, value_enc));
        }

        let raw_size: usize = encoded_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();

        // Compress parallel
        use rayon::prelude::*;
        let compressed_blocks: Vec<(Vec<u8>, Vec<u8>)> = encoded_blocks
            .par_iter()
            .map(|(time, value)| {
                let mut compressor = create_compressor(CompressionType::Lz4);
                let tc = compressor.compress(time).unwrap();
                let vc = compressor.compress(value).unwrap();
                (tc, vc)
            })
            .collect();

        let compressed_size: usize = compressed_blocks.iter()
            .map(|(t, v)| t.len() + v.len())
            .sum();
        let ratio = raw_size as f64 / compressed_size as f64;

        println!("DictionaryRLE+LZ4 Parallel {}pts: {}B → {}B ({:.2}x)",
                 size, raw_size, compressed_size, ratio);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &encoded_blocks,
            |b, blocks| {
                b.iter(|| {
                    use rayon::prelude::*;
                    let result: Vec<(Vec<u8>, Vec<u8>)> = blocks
                        .par_iter()
                        .map(|(time, value)| {
                            let mut compressor = create_compressor(CompressionType::Lz4);
                            let tc = compressor.compress(black_box(time)).unwrap();
                            let vc = compressor.compress(black_box(value)).unwrap();
                            (tc, vc)
                        })
                        .collect();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_quantized_zstd_serial,
    bench_quantized_lz4_parallel,
    bench_chimp128_zstd_serial,
    bench_chimp128_lz4_parallel,
    bench_dictionary_zstd_serial,
    bench_dictionary_lz4_parallel,
);
criterion_main!(benches);
