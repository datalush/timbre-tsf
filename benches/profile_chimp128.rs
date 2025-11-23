//! Profiling benchmark for Chimp128 F32 encoding
//!
//! This benchmark is designed to be profiled with perf to identify hotspots.
//!
//! Run with:
//!   cargo build --release --bench profile_chimp128
//!   sudo perf record -F 9999 -g ./target/release/deps/profile_chimp128-*
//!   sudo perf report --hierarchy -g graph,0.5,caller

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;

fn main() {
    const NUM_VALUES: usize = 1_000_000;
    const ITERATIONS: usize = 100;

    println!("Profiling Chimp128 F32 encoding ({} iterations, {} values each)...", ITERATIONS, NUM_VALUES);

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

    // Profile Chimp128 F32 with batch API
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut out = Vec::with_capacity(NUM_VALUES * 4);

    for _ in 0..ITERATIONS {
        encoder.encode_f32_batch(&sensor_f32, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();
        out.clear();
        encoder.reset();
    }

    println!("Done! Encoded {} total values", NUM_VALUES * ITERATIONS);
}
