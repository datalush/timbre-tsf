//! Profiling benchmark for Gorilla F32 decoding
//!
//! Run with:
//!   cargo build --release --bench profile_gorilla_decode
//!   sudo perf record -F 9999 -g ./target/release/deps/profile_gorilla_decode-*
//!   sudo perf report --hierarchy -g graph,0.5,caller

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_encoder, create_decoder};

fn main() {
    const NUM_VALUES: usize = 1_000_000;
    const ITERATIONS: usize = 100;

    println!("Profiling Gorilla F32 decoding ({} iterations, {} values each)...", ITERATIONS, NUM_VALUES);

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

    // Pre-encode the data
    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut encoded = Vec::new();
    encoder.encode_f32_batch(&sensor_f32, &mut encoded).unwrap();
    encoder.flush(&mut encoded).unwrap();

    println!("Encoded {} values into {} bytes", NUM_VALUES, encoded.len());

    // Profile decoding
    for _ in 0..ITERATIONS {
        let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut pos = 0;
        let mut output = Vec::with_capacity(NUM_VALUES);
        for _ in 0..NUM_VALUES {
            let val = decoder.read_f32(&encoded, &mut pos).unwrap();
            output.push(val);
        }
    }

    println!("Done! Decoded {} total values", NUM_VALUES * ITERATIONS);
}
