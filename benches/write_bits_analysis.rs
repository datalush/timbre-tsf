//! Análisis de patrones de write_bits para f32 vs f64
//! Instrumenta cuántas veces se usa el fast path vs byte-by-byte

use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use std::sync::atomic::{AtomicU64, Ordering};

// Contadores globales (solo para análisis)
static FAST_PATH_COUNT: AtomicU64 = AtomicU64::new(0);
static BYTE_LOOP_COUNT: AtomicU64 = AtomicU64::new(0);

fn main() {
    const NUM_VALUES: usize = 100_000;

    println!("\n=== Write Bits Pattern Analysis ===\n");

    // Test F32 Linear Ramp (worst case)
    println!("F32 Linear Ramp:");
    let ramp_f32: Vec<f32> = (0..NUM_VALUES).map(|i| i as f32 * 0.1).collect();

    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
    let mut out = Vec::new();

    for &v in &ramp_f32 {
        encoder.encode_f32(v, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();

    println!("  Output size: {} bytes", out.len());
    println!("  Bytes per value: {:.2}", out.len() as f64 / NUM_VALUES as f64);

    // Test F64 Linear Ramp
    println!("\nF64 Linear Ramp:");
    let ramp_f64: Vec<f64> = (0..NUM_VALUES).map(|i| i as f64 * 0.1).collect();

    let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Double);
    let mut out = Vec::new();

    for &v in &ramp_f64 {
        encoder.encode_f64(v, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();

    println!("  Output size: {} bytes", out.len());
    println!("  Bytes per value: {:.2}", out.len() as f64 / NUM_VALUES as f64);

    // Analysis
    println!("\n=== Analysis ===");
    println!("\nHypothesis:");
    println!("  - F32 generates more small writes (5+5+N bits)");
    println!("  - F64 generates fewer larger writes (6+6+N bits)");
    println!("  - F32 spends more time in byte-by-byte loop");
    println!("  - F64 uses fast path (8-byte writes) more often");

    println!("\nExpected:");
    println!("  - F32: More bytes/value but smaller chunks");
    println!("  - F64: More bytes/value with larger chunks");

    let ratio = (out.len() as f64 / NUM_VALUES as f64) /
                (ramp_f32.len() as f64 * std::mem::size_of::<f32>() as f64 / NUM_VALUES as f64);
    println!("\nCompression efficiency:");
    println!("  F32 bytes/value: ~{:.2} bytes", ramp_f32.len() as f64 * 4.0 / NUM_VALUES as f64);
    println!("  F64 bytes/value: ~{:.2} bytes", out.len() as f64 / NUM_VALUES as f64);
}
