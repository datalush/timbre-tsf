// Simple profiling binary for Chimp128 encoder
// Build with: cargo build --release --example profile_chimp128
// Profile with: sudo perf record -F 999 -g ./target/release/examples/profile_chimp128
// View report: sudo perf report

use timbre_tsf::common::TSDataType;
use timbre_tsf::encoding::{Chimp128Encoder, Encoder};

fn main() {
    // Generate test data: 1M f64 values with realistic time series patterns
    let data: Vec<f64> = (0..1_000_000)
        .map(|i| {
            let base = 25.0;
            let trend = (i as f64) * 0.0001;
            let noise = ((i as f64) * 0.1).sin() * 2.0;
            base + trend + noise
        })
        .collect();

    println!("Profiling Chimp128 encoder with {} values", data.len());
    println!("Running encoding 100 times for profiling...");

    // Run encoding multiple times to get good profiling samples
    for iteration in 0..100 {
        let mut encoder = Chimp128Encoder::new(TSDataType::Double);
        let mut out = Vec::with_capacity(data.len() * 8);

        for &value in &data {
            encoder.encode_f64(value, &mut out).unwrap();
        }

        encoder.flush(&mut out).unwrap();

        if iteration % 10 == 0 {
            println!("Iteration {}/100", iteration);
        }
    }

    println!("Profiling complete!");
}
