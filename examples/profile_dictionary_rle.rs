// Simple profiling binary for DictionaryRLE encoder
// Build with: cargo build --release --example profile_dictionary_rle
// Profile with: sudo perf record -F 999 -g ./target/release/examples/profile_dictionary_rle
// View report: sudo perf report

use timbre_tsf::encoding::dictionary_rle::DictionaryRLEEncoder;

fn main() {
    // Generate test data: 1M values with typical IoT repetition pattern
    // 85% repetition, 20 unique values (realistic for sensors)
    let unique_values: Vec<f64> = (0..20).map(|i| 20.0 + i as f64 * 0.1).collect();

    let mut data = Vec::with_capacity(1_000_000);
    let mut value_idx = 0;

    for i in 0..1_000_000 {
        data.push(unique_values[value_idx]);
        // Change value every ~15 points (85% repetition)
        if i % 15 == 0 && i > 0 {
            value_idx = (value_idx + 1) % unique_values.len();
        }
    }

    println!("Profiling DictionaryRLE encoder with {} values", data.len());
    println!("Running encoding 100 times for profiling...");

    // Run encoding multiple times to get good profiling samples
    for iteration in 0..100 {
        let mut encoder = DictionaryRLEEncoder::new();
        let encoded = encoder.encode(&data).unwrap();

        // Also decode to profile both paths
        let decoded = encoder.decode(&encoded).unwrap();

        // Verify correctness (first iteration only)
        if iteration == 0 {
            assert_eq!(data.len(), decoded.len(), "Decoded length mismatch");
            println!("Compression: {} bytes -> {} bytes ({:.2}x)",
                     data.len() * 8,
                     encoded.len(),
                     (data.len() * 8) as f64 / encoded.len() as f64);
        }

        if iteration % 10 == 0 {
            println!("Iteration {}/100", iteration);
        }
    }

    println!("Profiling complete!");
}
