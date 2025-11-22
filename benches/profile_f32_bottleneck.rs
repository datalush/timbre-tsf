//! Focused profiling benchmark to identify F32 vs F64 bottleneck
//!
//! This benchmark isolates the hot path in write_bits() to measure:
//! 1. Frequency of byte-by-byte writes vs batch writes (8 bytes)
//! 2. CPU time spent in each path
//! 3. Instruction-level differences between F32 and F64
//!
//! Run with:
//!   cargo build --release --bench profile_f32_bottleneck
//!   perf record -F 999 -g ./target/release/deps/profile_f32_bottleneck-*
//!   perf report --hierarchy -g graph,0.5,caller

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use std::time::Instant;

/// Measures encoding throughput in MiB/s
fn measure_throughput(label: &str, bytes: usize, iterations: usize, duration: std::time::Duration) {
    let total_bytes = bytes * iterations;
    let seconds = duration.as_secs_f64();
    let throughput_mb = (total_bytes as f64) / seconds / (1024.0 * 1024.0);
    println!("{:40} | {:8.2} MiB/s | {:8.3} ms/iter",
             label, throughput_mb, (seconds * 1000.0) / iterations as f64);
}

fn main() {
    println!("\n=== F32 vs F64 Bottleneck Analysis ===\n");
    println!("{:40} | {:>11} | {:>14}", "Test Case", "Throughput", "Latency");
    println!("{:-<40}-+-{:-<11}-+-{:-<14}", "", "", "");

    const NUM_VALUES: usize = 1_000_000;
    const ITERATIONS: usize = 50;

    // Pattern 1: Slowly changing sensor data (typical IoT use case)
    // This generates mostly small XORs with 20-40 bits of output per value
    let sensor_f32: Vec<f32> = {
        let mut values = Vec::with_capacity(NUM_VALUES);
        let mut val = 100.0f32;
        for i in 0..NUM_VALUES {
            val += (i as f32 * 0.001).sin() * 0.01; // Small fluctuations
            values.push(val);
        }
        values
    };

    let sensor_f64: Vec<f64> = sensor_f32.iter().map(|&x| x as f64).collect();

    // Pattern 2: Constant values (best case: 1 bit per value after first)
    let constant_f32 = vec![42.0f32; NUM_VALUES];
    let constant_f64 = vec![42.0f64; NUM_VALUES];

    // Pattern 3: Linear ramp (medium XOR size)
    let ramp_f32: Vec<f32> = (0..NUM_VALUES).map(|i| i as f32 * 0.1).collect();
    let ramp_f64: Vec<f64> = (0..NUM_VALUES).map(|i| i as f64 * 0.1).collect();

    // Pattern 4: Random-ish (worst case for compression)
    let random_f32: Vec<f32> = (0..NUM_VALUES).map(|i| {
        ((i as f32) * 1234.5678).sin() * 1000.0
    }).collect();
    let random_f64: Vec<f64> = random_f32.iter().map(|&x| x as f64).collect();

    // === GORILLA ENCODING TESTS ===
    println!("\n--- Gorilla: Sensor Data (typical IoT) ---");

    // F32 - Gorilla
    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &sensor_f32 {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f32 = start.elapsed();
    measure_throughput("Gorilla F32", total_bytes, ITERATIONS, duration_f32);

    // F64 - Gorilla
    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &sensor_f64 {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f64 = start.elapsed();
    measure_throughput("Gorilla F64", total_bytes, ITERATIONS, duration_f64);

    println!("  Ratio F64/F32: {:.2}x faster", duration_f32.as_secs_f64() / duration_f64.as_secs_f64());

    // === CONSTANT VALUES (best case) ===
    println!("\n--- Gorilla: Constant Values (1 bit/value) ---");

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &constant_f32 {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f32_const = start.elapsed();
    measure_throughput("Gorilla F32 (const)", total_bytes, ITERATIONS, duration_f32_const);

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &constant_f64 {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f64_const = start.elapsed();
    measure_throughput("Gorilla F64 (const)", total_bytes, ITERATIONS, duration_f64_const);

    println!("  Ratio F64/F32: {:.2}x faster", duration_f32_const.as_secs_f64() / duration_f64_const.as_secs_f64());

    // === LINEAR RAMP ===
    println!("\n--- Gorilla: Linear Ramp ---");

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &ramp_f32 {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f32_ramp = start.elapsed();
    measure_throughput("Gorilla F32 (ramp)", total_bytes, ITERATIONS, duration_f32_ramp);

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &ramp_f64 {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f64_ramp = start.elapsed();
    measure_throughput("Gorilla F64 (ramp)", total_bytes, ITERATIONS, duration_f64_ramp);

    println!("  Ratio F64/F32: {:.2}x faster", duration_f32_ramp.as_secs_f64() / duration_f64_ramp.as_secs_f64());

    // === RANDOM (worst case) ===
    println!("\n--- Gorilla: Random Values (worst compression) ---");

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &random_f32 {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f32_random = start.elapsed();
    measure_throughput("Gorilla F32 (random)", total_bytes, ITERATIONS, duration_f32_random);

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &random_f64 {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_f64_random = start.elapsed();
    measure_throughput("Gorilla F64 (random)", total_bytes, ITERATIONS, duration_f64_random);

    println!("  Ratio F64/F32: {:.2}x faster", duration_f32_random.as_secs_f64() / duration_f64_random.as_secs_f64());

    // === CHIMP128 ENCODING TESTS ===
    println!("\n--- Chimp128: Sensor Data (typical IoT) ---");

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &sensor_f32 {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_chimp_f32 = start.elapsed();
    measure_throughput("Chimp128 F32", total_bytes, ITERATIONS, duration_chimp_f32);

    let start = Instant::now();
    let mut total_bytes = 0;
    for _ in 0..ITERATIONS {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &sensor_f64 {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_bytes = out.len();
    }
    let duration_chimp_f64 = start.elapsed();
    measure_throughput("Chimp128 F64", total_bytes, ITERATIONS, duration_chimp_f64);

    println!("  Ratio F64/F32: {:.2}x faster", duration_chimp_f32.as_secs_f64() / duration_chimp_f64.as_secs_f64());

    // === SUMMARY ===
    println!("\n=== SUMMARY ===");
    println!("\nGorilla:");
    println!("  Sensor:   F64 is {:.2}x faster than F32", duration_f32.as_secs_f64() / duration_f64.as_secs_f64());
    println!("  Constant: F64 is {:.2}x faster than F32", duration_f32_const.as_secs_f64() / duration_f64_const.as_secs_f64());
    println!("  Ramp:     F64 is {:.2}x faster than F32", duration_f32_ramp.as_secs_f64() / duration_f64_ramp.as_secs_f64());
    println!("  Random:   F64 is {:.2}x faster than F32", duration_f32_random.as_secs_f64() / duration_f64_random.as_secs_f64());
    println!("\nChimp128:");
    println!("  Sensor:   F64 is {:.2}x faster than F32", duration_chimp_f32.as_secs_f64() / duration_chimp_f64.as_secs_f64());

    println!("\nHypothesis Test:");
    println!("  If F32 is slower due to byte-by-byte writes in write_bits(),");
    println!("  the constant pattern should show the BIGGEST gap (1 bit writes)");
    println!("  and random pattern should show SMALLER gap (full 32/64 bit writes).");
    println!("\n  Actual constant gap: {:.2}x", duration_f32_const.as_secs_f64() / duration_f64_const.as_secs_f64());
    println!("  Actual random gap:   {:.2}x", duration_f32_random.as_secs_f64() / duration_f64_random.as_secs_f64());

    if (duration_f32_const.as_secs_f64() / duration_f64_const.as_secs_f64()) >
       (duration_f32_random.as_secs_f64() / duration_f64_random.as_secs_f64()) {
        println!("\n  ✓ Hypothesis CONFIRMED: Byte-by-byte writes are the bottleneck!");
    } else {
        println!("\n  ✗ Hypothesis REJECTED: Bottleneck is elsewhere!");
    }
}
