//! Comprehensive comparison benchmark: Gorilla vs Chimp128 with realistic batch API
//!
//! This benchmark tests BOTH encoders using the production pattern:
//! - Batch encoding API (encode_f32_batch/encode_f64_batch)
//! - Encoder reuse with reset()
//! - Pre-allocated buffers
//!
//! Run with:
//!   cargo build --release --bench profile_comparison
//!   /home/midnattsol/code/iot/timbre-tsf/target/release/deps/profile_comparison-*

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
    println!("\n=== GORILLA vs CHIMP128 Comprehensive Benchmark (Batch API + Reuse) ===\n");
    println!("{:40} | {:>11} | {:>14} | {:>11}", "Test Case", "Throughput", "Values/sec", "Latency");
    println!("{:-<40}-+-{:-<11}-+-{:-<14}-+-{:-<11}", "", "", "", "");

    const NUM_VALUES: usize = 1_000_000;
    const ITERATIONS: usize = 50;

    // Pattern 1: Slowly changing sensor data (typical IoT use case)
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

    // Pattern 2: Constant values (best case compression)
    let constant_f32 = vec![42.0f32; NUM_VALUES];
    let constant_f64 = vec![42.0f64; NUM_VALUES];

    // Pattern 3: Linear ramp
    let ramp_f32: Vec<f32> = (0..NUM_VALUES).map(|i| i as f32 * 0.1).collect();
    let ramp_f64: Vec<f64> = (0..NUM_VALUES).map(|i| i as f64 * 0.1).collect();

    // Pattern 4: Random-ish (worst case)
    let random_f32: Vec<f32> = (0..NUM_VALUES).map(|i| {
        ((i as f32) * 1234.5678).sin() * 1000.0
    }).collect();
    let random_f64: Vec<f64> = random_f32.iter().map(|&x| x as f64).collect();

    // ============================================================================
    // GORILLA: SENSOR DATA
    // ============================================================================
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
    let duration_gorilla_f32_sensor = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_sensor);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&sensor_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f64_sensor = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_gorilla_f64_sensor);

    println!("  Ratio F64/F32: {:.2}x", duration_gorilla_f32_sensor.as_secs_f64() / duration_gorilla_f64_sensor.as_secs_f64());

    // ============================================================================
    // CHIMP128: SENSOR DATA
    // ============================================================================
    println!("\n--- Chimp128: Sensor Data (Batch API) ---");

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&sensor_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f32_sensor = start.elapsed();
    measure_throughput("Chimp128 F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_sensor);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&sensor_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f64_sensor = start.elapsed();
    measure_throughput("Chimp128 F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_chimp_f64_sensor);

    println!("  Ratio F64/F32: {:.2}x", duration_chimp_f32_sensor.as_secs_f64() / duration_chimp_f64_sensor.as_secs_f64());

    // ============================================================================
    // CONSTANT VALUES
    // ============================================================================
    println!("\n--- Constant Values (Batch API) ---");

    // Gorilla
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&constant_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f32_const = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_const);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&constant_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f64_const = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_gorilla_f64_const);

    // Chimp128
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&constant_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f32_const = start.elapsed();
    measure_throughput("Chimp128 F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_const);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&constant_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f64_const = start.elapsed();
    measure_throughput("Chimp128 F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_chimp_f64_const);

    // ============================================================================
    // LINEAR RAMP
    // ============================================================================
    println!("\n--- Linear Ramp (Batch API) ---");

    // Gorilla
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&ramp_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f32_ramp = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_ramp);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&ramp_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f64_ramp = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_gorilla_f64_ramp);

    // Chimp128
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&ramp_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f32_ramp = start.elapsed();
    measure_throughput("Chimp128 F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_ramp);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&ramp_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f64_ramp = start.elapsed();
    measure_throughput("Chimp128 F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_chimp_f64_ramp);

    // ============================================================================
    // RANDOM VALUES
    // ============================================================================
    println!("\n--- Random Values (Batch API) ---");

    // Gorilla
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&random_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f32_random = start.elapsed();
    measure_throughput("Gorilla F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_random);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&random_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_gorilla_f64_random = start.elapsed();
    measure_throughput("Gorilla F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_gorilla_f64_random);

    // Chimp128
    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);
    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&random_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f32_random = start.elapsed();
    measure_throughput("Chimp128 F32 (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_random);

    let start = Instant::now();
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut out = Vec::with_capacity(NUM_VALUES * 8);
    for _ in 0..ITERATIONS {
        encoder.encode_f64_batch(&random_f64, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }
    let duration_chimp_f64_random = start.elapsed();
    measure_throughput("Chimp128 F64 (batch)", NUM_VALUES, 8, ITERATIONS, duration_chimp_f64_random);

    // ============================================================================
    // SUMMARY
    // ============================================================================
    println!("\n=== SUMMARY (Batch API + Reuse) ===");

    println!("\n--- Gorilla Performance ---");
    println!("  Sensor:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_sensor.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_sensor.as_secs_f64() / 1e6,
             if duration_gorilla_f32_sensor < duration_gorilla_f64_sensor { "F32" } else { "F64" });

    println!("  Constant:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_const.as_secs_f64() / 1e6,
             if duration_gorilla_f32_const < duration_gorilla_f64_const { "F32" } else { "F64" });

    println!("  Ramp:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_ramp.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_ramp.as_secs_f64() / 1e6,
             if duration_gorilla_f32_ramp < duration_gorilla_f64_ramp { "F32" } else { "F64" });

    println!("  Random:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_random.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_random.as_secs_f64() / 1e6,
             if duration_gorilla_f32_random < duration_gorilla_f64_random { "F32" } else { "F64" });

    println!("\n--- Chimp128 Performance ---");
    println!("  Sensor:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_sensor.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_sensor.as_secs_f64() / 1e6,
             if duration_chimp_f32_sensor < duration_chimp_f64_sensor { "F32" } else { "F64" });

    println!("  Constant:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_const.as_secs_f64() / 1e6,
             if duration_chimp_f32_const < duration_chimp_f64_const { "F32" } else { "F64" });

    println!("  Ramp:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_ramp.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_ramp.as_secs_f64() / 1e6,
             if duration_chimp_f32_ramp < duration_chimp_f64_ramp { "F32" } else { "F64" });

    println!("  Random:");
    println!("    F32: {:.1}M val/s | F64: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_random.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_random.as_secs_f64() / 1e6,
             if duration_chimp_f32_random < duration_chimp_f64_random { "F32" } else { "F64" });

    println!("\n--- Gorilla vs Chimp128 (F32) ---");
    println!("  Sensor:   Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_sensor.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_sensor.as_secs_f64() / 1e6,
             if duration_gorilla_f32_sensor < duration_chimp_f32_sensor { "Gorilla" } else { "Chimp128" });

    println!("  Constant: Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_const.as_secs_f64() / 1e6,
             if duration_gorilla_f32_const < duration_chimp_f32_const { "Gorilla" } else { "Chimp128" });

    println!("  Ramp:     Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_ramp.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_ramp.as_secs_f64() / 1e6,
             if duration_gorilla_f32_ramp < duration_chimp_f32_ramp { "Gorilla" } else { "Chimp128" });

    println!("  Random:   Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_random.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_random.as_secs_f64() / 1e6,
             if duration_gorilla_f32_random < duration_chimp_f32_random { "Gorilla" } else { "Chimp128" });

    println!("\n--- Gorilla vs Chimp128 (F64) ---");
    println!("  Sensor:   Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_sensor.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_sensor.as_secs_f64() / 1e6,
             if duration_gorilla_f64_sensor < duration_chimp_f64_sensor { "Gorilla" } else { "Chimp128" });

    println!("  Constant: Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_const.as_secs_f64() / 1e6,
             if duration_gorilla_f64_const < duration_chimp_f64_const { "Gorilla" } else { "Chimp128" });

    println!("  Ramp:     Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_ramp.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_ramp.as_secs_f64() / 1e6,
             if duration_gorilla_f64_ramp < duration_chimp_f64_ramp { "Gorilla" } else { "Chimp128" });

    println!("  Random:   Gorilla: {:.1}M val/s | Chimp128: {:.1}M val/s | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_random.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_random.as_secs_f64() / 1e6,
             if duration_gorilla_f64_random < duration_chimp_f64_random { "Gorilla" } else { "Chimp128" });
}
