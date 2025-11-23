//! Realistic benchmark using batch API and encoder reuse (like production)

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use std::time::Instant;

fn measure_throughput(label: &str, num_values: usize, value_size: usize, iterations: usize, duration: std::time::Duration) {
    let input_bytes_per_iter = num_values * value_size;
    let total_input_bytes = input_bytes_per_iter * iterations;
    let seconds = duration.as_secs_f64();
    let throughput_mb = (total_input_bytes as f64) / seconds / (1024.0 * 1024.0);
    let values_per_sec = (num_values * iterations) as f64 / seconds;
    println!("{:40} | {:8.2} MiB/s | {:10.0} val/s | {:8.3} ms/iter",
             label, throughput_mb, values_per_sec, (seconds * 1000.0) / iterations as f64);
}

fn main() {
    println!("\n=== REALISTIC Gorilla Benchmark (Batch API + Reuse) ===\n");
    println!("{:40} | {:>11} | {:>14} | {:>11}", "Test Case", "Throughput", "Values/sec", "Latency");
    println!("{:-<40}-+-{:-<11}-+-{:-<14}-+-{:-<11}", "", "", "", "");

    const NUM_VALUES: usize = 1_000_000;
    const ITERATIONS: usize = 50;

    // Pattern 1: Slowly changing sensor data
    let sensor_f32: Vec<f32> = {
        let mut values = Vec::with_capacity(NUM_VALUES);
        let mut val = 100.0f32;
        for i in 0..NUM_VALUES {
            val += (i as f32 * 0.001).sin() * 0.01;
            values.push(val);
        }
        values
    };
    let sensor_f64: Vec<f64> = sensor_f32.iter().map(|&x| x as f64).collect();

    // Pattern 2: Constant values
    let constant_f32 = vec![42.0f32; NUM_VALUES];
    let constant_f64 = vec![42.0f64; NUM_VALUES];

    // Pattern 3: Linear ramp
    let ramp_f32: Vec<f32> = (0..NUM_VALUES).map(|i| i as f32 * 0.1).collect();
    let ramp_f64: Vec<f64> = (0..NUM_VALUES).map(|i| i as f64 * 0.1).collect();

    // Pattern 4: Random
    let random_f32: Vec<f32> = (0..NUM_VALUES).map(|i| {
        ((i as f32) * 1234.5678).sin() * 1000.0
    }).collect();
    let random_f64: Vec<f64> = random_f32.iter().map(|&x| x as f64).collect();

    // === GORILLA F32 vs F64 (BATCH API) ===
    println!("\n--- Gorilla: Sensor Data (Batch API) ---");

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&sensor_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f32 = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_f32);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&sensor_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f64 = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_f64);

    println!("  Ratio F64/F32: {:.2}x", duration_f32.as_secs_f64() / duration_f64.as_secs_f64());

    // === CONSTANT ===
    println!("\n--- Gorilla: Constant Values (Batch API) ---");

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&constant_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f32_const = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_f32_const);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&constant_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f64_const = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_f64_const);

    println!("  Ratio F64/F32: {:.2}x", duration_f32_const.as_secs_f64() / duration_f64_const.as_secs_f64());

    // === LINEAR RAMP ===
    println!("\n--- Gorilla: Linear Ramp (Batch API) ---");

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&ramp_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f32_ramp = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_f32_ramp);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&ramp_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f64_ramp = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_f64_ramp);

    println!("  Ratio F64/F32: {:.2}x", duration_f32_ramp.as_secs_f64() / duration_f64_ramp.as_secs_f64());

    // === RANDOM ===
    println!("\n--- Gorilla: Random Values (Batch API) ---");

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&random_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f32_random = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_f32_random);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&random_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_f64_random = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_f64_random);

    println!("  Ratio F64/F32: {:.2}x", duration_f32_random.as_secs_f64() / duration_f64_random.as_secs_f64());

    // === SUMMARY ===
    println!("\n=== SUMMARY (Batch API + Reuse) ===");
    println!("Gorilla:");
    println!("  Sensor:   F32: {:.1}M val/s  vs  F64: {:.1}M val/s",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f32.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f64.as_secs_f64() / 1e6);
    println!("  Constant: F32: {:.1}M val/s  vs  F64: {:.1}M val/s",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f32_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f64_const.as_secs_f64() / 1e6);
    println!("  Ramp:     F32: {:.1}M val/s  vs  F64: {:.1}M val/s",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f32_ramp.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f64_ramp.as_secs_f64() / 1e6);
    println!("  Random:   F32: {:.1}M val/s  vs  F64: {:.1}M val/s",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f32_random.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_f64_random.as_secs_f64() / 1e6);
}
