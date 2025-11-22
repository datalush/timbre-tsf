//! Profiling benchmark to compare f32 vs f64 encoding performance
//! Run with: cargo flamegraph --bench profile_comparison

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;

fn main() {
    let iterations = 1000;
    let size = 100_000;

    // Generate f32 data
    let mut f32_values = Vec::with_capacity(size);
    let mut val = 100.0f32;
    for i in 0..size {
        val += (i as f32 * 0.001).sin() * 0.01;
        f32_values.push(val);
    }

    // Generate f64 data
    let mut f64_values = Vec::with_capacity(size);
    let mut val = 100.0f64;
    for i in 0..size {
        val += (i as f64 * 0.001).sin() * 0.01;
        f64_values.push(val);
    }

    println!("Profiling F32 Gorilla encoding ({} iterations, {} values)...", iterations, size);
    for _ in 0..iterations {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();
        for &v in &f32_values {
            encoder.encode_f32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
    }

    println!("Profiling F64 Gorilla encoding ({} iterations, {} values)...", iterations, size);
    for _ in 0..iterations {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
        let mut out = Vec::new();
        for &v in &f64_values {
            encoder.encode_f64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
    }

    println!("Done!");
}
