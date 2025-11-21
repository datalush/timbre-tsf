use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::time::Duration;
use timbre_tsf::encoding::simd::{xor_detect_identical, count_leading_zeros_batch, count_trailing_zeros_batch, features};

/// Generate realistic sensor data with slow variation
fn generate_sensor_data(n: usize) -> Vec<f32> {
    let mut data = Vec::with_capacity(n);
    let mut value = 20.0f32;

    for i in 0..n {
        // Simulate slow sensor drift with occasional spikes
        if i % 100 == 0 {
            value += 0.5; // Drift
        }
        if i % 1000 == 0 {
            value += 2.0; // Spike
        }

        data.push(value + (i as f32 * 0.001) % 0.1);
    }

    data
}

/// Benchmark XOR + identical detection (critical path for Gorilla/Chimp128)
fn benchmark_xor_detection(c: &mut Criterion) {
    println!("SIMD capabilities: {}", features::simd_capabilities());

    let sizes = [1_000, 10_000, 100_000, 1_000_000];

    let mut group = c.benchmark_group("xor_detection");

    for &size in &sizes {
        let current = generate_sensor_data(size);
        let previous = generate_sensor_data(size - 1);

        // Throughput: measure MB/s
        group.throughput(Throughput::Bytes((size * 4) as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut total_identical = 0u32;

                    // Process in batches of 8
                    for i in (0..size.saturating_sub(8)).step_by(8) {
                        let curr_batch: [f32; 8] = current[i..i+8].try_into().unwrap();
                        let prev_batch: [f32; 8] = previous[i..i+8].try_into().unwrap();

                        let result = xor_detect_identical(
                            black_box(&curr_batch),
                            black_box(&prev_batch)
                        );

                        // Count identical values
                        total_identical += result.identical_mask.count_ones();
                    }

                    total_identical
                });
            },
        );
    }

    group.finish();
}

/// Benchmark leading zeros counting (bottleneck for SIMD)
fn benchmark_leading_zeros(c: &mut Criterion) {
    let sizes = [1_000, 10_000, 100_000, 1_000_000];

    let mut group = c.benchmark_group("leading_zeros");

    for &size in &sizes {
        let data = generate_sensor_data(size);

        // Convert to u32 bits
        let bits: Vec<u32> = data.iter().map(|&f| f.to_bits()).collect();

        group.throughput(Throughput::Bytes((size * 4) as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut total = 0u32;

                    // Process in batches of 8
                    for i in (0..size.saturating_sub(8)).step_by(8) {
                        let batch: [u32; 8] = bits[i..i+8].try_into().unwrap();
                        let lz = count_leading_zeros_batch(black_box(&batch));

                        total += lz.iter().map(|&x| x as u32).sum::<u32>();
                    }

                    total
                });
            },
        );
    }

    group.finish();
}

/// Benchmark trailing zeros counting
fn benchmark_trailing_zeros(c: &mut Criterion) {
    let sizes = [1_000, 10_000, 100_000, 1_000_000];

    let mut group = c.benchmark_group("trailing_zeros");

    for &size in &sizes {
        let data = generate_sensor_data(size);
        let bits: Vec<u32> = data.iter().map(|&f| f.to_bits()).collect();

        group.throughput(Throughput::Bytes((size * 4) as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut total = 0u32;

                    for i in (0..size.saturating_sub(8)).step_by(8) {
                        let batch: [u32; 8] = bits[i..i+8].try_into().unwrap();
                        let tz = count_trailing_zeros_batch(black_box(&batch));

                        total += tz.iter().map(|&x| x as u32).sum::<u32>();
                    }

                    total
                });
            },
        );
    }

    group.finish();
}

/// Compare SIMD vs scalar for full encoding pipeline simulation
fn benchmark_encoding_simulation(c: &mut Criterion) {
    let sizes = [10_000, 100_000, 1_000_000];

    let mut group = c.benchmark_group("encoding_simulation");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    for &size in &sizes {
        let current = generate_sensor_data(size);
        let previous = generate_sensor_data(size - 1);

        group.throughput(Throughput::Bytes((size * 4) as u64));

        // Simulate Chimp128/Gorilla encoding pipeline with SIMD
        group.bench_with_input(
            BenchmarkId::new("simd", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut encoded_bits = 0u64;

                    for i in (0..size.saturating_sub(8)).step_by(8) {
                        let curr_batch: [f32; 8] = current[i..i+8].try_into().unwrap();
                        let prev_batch: [f32; 8] = previous[i..i+8].try_into().unwrap();

                        // SIMD: XOR + detect identical
                        let result = xor_detect_identical(
                            black_box(&curr_batch),
                            black_box(&prev_batch)
                        );

                        // Write control bits (1 byte for 8 values)
                        encoded_bits += 8;

                        // Scalar: leading/trailing zeros for non-identical values
                        let lz = count_leading_zeros_batch(&result.xors);
                        let tz = count_trailing_zeros_batch(&result.xors);

                        for j in 0..8 {
                            if (result.identical_mask & (1 << j)) == 0 {
                                // Value changed - encode
                                let significant = 32 - lz[j] - tz[j];
                                encoded_bits += significant as u64;
                            }
                        }
                    }

                    encoded_bits
                });
            },
        );

        // Simulate scalar encoding (baseline)
        group.bench_with_input(
            BenchmarkId::new("scalar", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut encoded_bits = 0u64;

                    for i in 0..size.saturating_sub(1) {
                        let curr = black_box(current[i].to_bits());
                        let prev = black_box(previous[i].to_bits());

                        let xor = curr ^ prev;

                        if xor == 0 {
                            // Identical
                            encoded_bits += 1;
                        } else {
                            // Changed
                            encoded_bits += 1; // control bit

                            let lz = xor.leading_zeros() as u8;
                            let tz = xor.trailing_zeros() as u8;
                            let significant = 32 - lz - tz;

                            encoded_bits += significant as u64;
                        }
                    }

                    encoded_bits
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_xor_detection,
    benchmark_leading_zeros,
    benchmark_trailing_zeros,
    benchmark_encoding_simulation
);
criterion_main!(benches);
