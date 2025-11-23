//! Focused profiling benchmark for F32 decoding ONLY
//!
//! Run with:
//!   cargo build --release --bench profile_decode_only
//!   sudo perf record -F 9999 -g ./target/release/deps/profile_decode_only-*
//!   sudo perf report --hierarchy -g graph,0.5,caller
//!
//! This benchmark isolates the decoding loop from encoding to make perf analysis clearer

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_encoder, create_decoder};

fn setup_encoded_data() -> Vec<u8> {
    const NUM_VALUES: usize = 1_000_000;

    // Sensor data pattern (most realistic for IoT)
    let sensor_f32: Vec<f32> = {
        let mut values = Vec::with_capacity(NUM_VALUES);
        let mut val = 100.0f32;
        for i in 0..NUM_VALUES {
            val += (i as f32 * 0.001).sin() * 0.01;
            values.push(val);
        }
        values
    };

    // Encode once
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut encoded = Vec::new();
    encoder.encode_f32_batch(&sensor_f32, &mut encoded).unwrap();
    encoder.flush(&mut encoded).unwrap();

    encoded
}

#[inline(never)]
fn decode_benchmark(encoded: &[u8], iterations: usize) -> usize {
    const NUM_VALUES: usize = 1_000_000;
    let mut total = 0;

    for _ in 0..iterations {
        let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);

        // Use batch API for better performance
        decoder.read_f32_batch(encoded, &mut pos, &mut output, NUM_VALUES).unwrap();

        total += output.len();
    }

    total
}

fn main() {
    const ITERATIONS: usize = 100;

    println!("Setting up encoded data...");
    let encoded = setup_encoded_data();
    println!("Encoded data size: {} bytes", encoded.len());

    println!("Profiling decoding ONLY ({} iterations)...", ITERATIONS);
    let total = decode_benchmark(&encoded, ITERATIONS);

    println!("Done! Decoded {} total values", total);
}
