//! Comprehensive decoding benchmark: Gorilla vs Chimp128
//!
//! This benchmark tests BOTH decoders using realistic patterns:
//! - Batch decoding API (read_f32_batch/read_f64_batch)
//! - Pre-encoded data (realistic scenario)
//! - Multiple data patterns
//!
//! Run with:
//!   cargo build --release --bench profile_decoding
//!   /home/midnattsol/code/iot/timbre-tsf/target/release/deps/profile_decoding-*

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_encoder, create_decoder};
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
    println!("\n=== DECODING Benchmark: Gorilla vs Chimp128 (Batch API) ===\n");
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
    // PRE-ENCODE DATA (this is not measured, just setup)
    // ============================================================================

    // Gorilla F32 - Sensor
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut gorilla_f32_sensor_encoded = Vec::new();
    encoder.encode_f32_batch(&sensor_f32, &mut gorilla_f32_sensor_encoded).unwrap();
    encoder.flush(&mut gorilla_f32_sensor_encoded).unwrap();

    // Gorilla F64 - Sensor
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut gorilla_f64_sensor_encoded = Vec::new();
    encoder.encode_f64_batch(&sensor_f64, &mut gorilla_f64_sensor_encoded).unwrap();
    encoder.flush(&mut gorilla_f64_sensor_encoded).unwrap();

    // Gorilla F32 - Constant
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut gorilla_f32_const_encoded = Vec::new();
    encoder.encode_f32_batch(&constant_f32, &mut gorilla_f32_const_encoded).unwrap();
    encoder.flush(&mut gorilla_f32_const_encoded).unwrap();

    // Gorilla F64 - Constant
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut gorilla_f64_const_encoded = Vec::new();
    encoder.encode_f64_batch(&constant_f64, &mut gorilla_f64_const_encoded).unwrap();
    encoder.flush(&mut gorilla_f64_const_encoded).unwrap();

    // Gorilla F32 - Ramp
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut gorilla_f32_ramp_encoded = Vec::new();
    encoder.encode_f32_batch(&ramp_f32, &mut gorilla_f32_ramp_encoded).unwrap();
    encoder.flush(&mut gorilla_f32_ramp_encoded).unwrap();

    // Gorilla F64 - Ramp
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut gorilla_f64_ramp_encoded = Vec::new();
    encoder.encode_f64_batch(&ramp_f64, &mut gorilla_f64_ramp_encoded).unwrap();
    encoder.flush(&mut gorilla_f64_ramp_encoded).unwrap();

    // Gorilla F32 - Random
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut gorilla_f32_random_encoded = Vec::new();
    encoder.encode_f32_batch(&random_f32, &mut gorilla_f32_random_encoded).unwrap();
    encoder.flush(&mut gorilla_f32_random_encoded).unwrap();

    // Gorilla F64 - Random
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut gorilla_f64_random_encoded = Vec::new();
    encoder.encode_f64_batch(&random_f64, &mut gorilla_f64_random_encoded).unwrap();
    encoder.flush(&mut gorilla_f64_random_encoded).unwrap();

    // Chimp128 F32 - Sensor
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut chimp_f32_sensor_encoded = Vec::new();
    encoder.encode_f32_batch(&sensor_f32, &mut chimp_f32_sensor_encoded).unwrap();
    encoder.flush(&mut chimp_f32_sensor_encoded).unwrap();

    // Chimp128 F64 - Sensor
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut chimp_f64_sensor_encoded = Vec::new();
    encoder.encode_f64_batch(&sensor_f64, &mut chimp_f64_sensor_encoded).unwrap();
    encoder.flush(&mut chimp_f64_sensor_encoded).unwrap();

    // Chimp128 F32 - Constant
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut chimp_f32_const_encoded = Vec::new();
    encoder.encode_f32_batch(&constant_f32, &mut chimp_f32_const_encoded).unwrap();
    encoder.flush(&mut chimp_f32_const_encoded).unwrap();

    // Chimp128 F64 - Constant
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut chimp_f64_const_encoded = Vec::new();
    encoder.encode_f64_batch(&constant_f64, &mut chimp_f64_const_encoded).unwrap();
    encoder.flush(&mut chimp_f64_const_encoded).unwrap();

    // Chimp128 F32 - Ramp
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut chimp_f32_ramp_encoded = Vec::new();
    encoder.encode_f32_batch(&ramp_f32, &mut chimp_f32_ramp_encoded).unwrap();
    encoder.flush(&mut chimp_f32_ramp_encoded).unwrap();

    // Chimp128 F64 - Ramp
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut chimp_f64_ramp_encoded = Vec::new();
    encoder.encode_f64_batch(&ramp_f64, &mut chimp_f64_ramp_encoded).unwrap();
    encoder.flush(&mut chimp_f64_ramp_encoded).unwrap();

    // Chimp128 F32 - Random
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut chimp_f32_random_encoded = Vec::new();
    encoder.encode_f32_batch(&random_f32, &mut chimp_f32_random_encoded).unwrap();
    encoder.flush(&mut chimp_f32_random_encoded).unwrap();

    // Chimp128 F64 - Random
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut chimp_f64_random_encoded = Vec::new();
    encoder.encode_f64_batch(&random_f64, &mut chimp_f64_random_encoded).unwrap();
    encoder.flush(&mut chimp_f64_random_encoded).unwrap();

    // ============================================================================
    // GORILLA DECODING: SENSOR DATA
    // ============================================================================
    println!("\n--- Gorilla: Sensor Data (Decoding) ---");

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f32_batch(&gorilla_f32_sensor_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_gorilla_f32_sensor = start.elapsed();
    measure_throughput("Gorilla F32 decode (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_sensor);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f64_batch(&gorilla_f64_sensor_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_gorilla_f64_sensor = start.elapsed();
    measure_throughput("Gorilla F64 decode (batch)", NUM_VALUES, 8, ITERATIONS, duration_gorilla_f64_sensor);

    println!("  Ratio F64/F32: {:.2}x", duration_gorilla_f32_sensor.as_secs_f64() / duration_gorilla_f64_sensor.as_secs_f64());

    // ============================================================================
    // CHIMP128 DECODING: SENSOR DATA
    // ============================================================================
    println!("\n--- Chimp128: Sensor Data (Decoding) ---");

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f32_batch(&chimp_f32_sensor_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_chimp_f32_sensor = start.elapsed();
    measure_throughput("Chimp128 F32 decode (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_sensor);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Double);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f64_batch(&chimp_f64_sensor_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_chimp_f64_sensor = start.elapsed();
    measure_throughput("Chimp128 F64 decode (batch)", NUM_VALUES, 8, ITERATIONS, duration_chimp_f64_sensor);

    println!("  Ratio F64/F32: {:.2}x", duration_chimp_f32_sensor.as_secs_f64() / duration_chimp_f64_sensor.as_secs_f64());

    // ============================================================================
    // CONSTANT VALUES DECODING
    // ============================================================================
    println!("\n--- Constant Values (Decoding) ---");

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f32_batch(&gorilla_f32_const_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_gorilla_f32_const = start.elapsed();
    measure_throughput("Gorilla F32 decode (batch)", NUM_VALUES, 4, ITERATIONS, duration_gorilla_f32_const);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        decoder.read_f32_batch(&chimp_f32_const_encoded, &mut pos, &mut output, NUM_VALUES).unwrap();
    }
    let duration_chimp_f32_const = start.elapsed();
    measure_throughput("Chimp128 F32 decode (batch)", NUM_VALUES, 4, ITERATIONS, duration_chimp_f32_const);

    // ============================================================================
    // SUMMARY
    // ============================================================================
    println!("\n=== DECODING SUMMARY ===");

    println!("\n--- Gorilla Decoding Performance ---");
    println!("  Sensor F32: {:.1}M val/s", NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_sensor.as_secs_f64() / 1e6);
    println!("  Sensor F64: {:.1}M val/s", NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f64_sensor.as_secs_f64() / 1e6);

    println!("\n--- Chimp128 Decoding Performance ---");
    println!("  Sensor F32: {:.1}M val/s", NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_sensor.as_secs_f64() / 1e6);
    println!("  Sensor F64: {:.1}M val/s", NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f64_sensor.as_secs_f64() / 1e6);

    println!("\n--- Gorilla vs Chimp128 (Decoding) ---");
    println!("  Sensor F32: Gorilla {:.1}M vs Chimp128 {:.1}M | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_sensor.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_sensor.as_secs_f64() / 1e6,
             if duration_gorilla_f32_sensor < duration_chimp_f32_sensor { "Gorilla" } else { "Chimp128" });

    println!("  Constant F32: Gorilla {:.1}M vs Chimp128 {:.1}M | Winner: {}",
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_gorilla_f32_const.as_secs_f64() / 1e6,
             NUM_VALUES as f64 * ITERATIONS as f64 / duration_chimp_f32_const.as_secs_f64() / 1e6,
             if duration_gorilla_f32_const < duration_chimp_f32_const { "Gorilla" } else { "Chimp128" });
}
