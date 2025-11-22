//! Simple benchmark for profiling f32 encoding performance
//! Run with: cargo bench --bench profile_f32 --no-run
//! Then: perf record -g target/release/deps/profile_f32-* --bench

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;

fn main() {
    // Generate 1M f32 values
    let mut values = Vec::with_capacity(1_000_000);
    let mut val = 100.0f32;
    for i in 0..1_000_000 {
        val += (i as f32 * 0.001).sin() * 0.01;
        values.push(val);
    }

    println!("Profiling Gorilla f32 encoding...");

    // Run multiple iterations for better profiling data
    for _ in 0..100 {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &values {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
    }

    println!("Profiling Chimp128 f32 encoding...");

    for _ in 0..100 {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &values {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
    }

    println!("Done!");
}
