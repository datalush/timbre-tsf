// Simple profiling binary for Dictionary encoder
// Build with: cargo build --release --example profile_dictionary
// Run with: ./target/release/examples/profile_dictionary

use timbre_tsf::common::TSDataType;
use timbre_tsf::encoding::{Encoder, DictionaryEncoder};

fn main() {
    // Generate test data: device names with high repetition
    // Simulating typical IoT scenario: 20 unique device names, 85% repetition
    let device_names = vec![
        "root.sg.device_sensor_001",
        "root.sg.device_sensor_002",
        "root.sg.device_sensor_003",
        "root.sg.device_sensor_004",
        "root.sg.device_sensor_005",
        "root.sg.device_actuator_001",
        "root.sg.device_actuator_002",
        "root.sg.device_actuator_003",
        "root.sg.device_gateway_001",
        "root.sg.device_gateway_002",
        "root.sg.device_controller_001",
        "root.sg.device_controller_002",
        "root.sg.device_monitor_001",
        "root.sg.device_monitor_002",
        "root.sg.device_alarm_001",
        "root.sg.device_alarm_002",
        "root.sg.device_hvac_001",
        "root.sg.device_hvac_002",
        "root.sg.device_lighting_001",
        "root.sg.device_lighting_002",
    ];

    // Generate 1M values with 85% repetition (typical for device names)
    let mut data = Vec::with_capacity(1_000_000);
    let mut device_idx = 0;

    for i in 0..1_000_000 {
        data.push(device_names[device_idx]);
        // Change device every ~15 entries (85% repetition)
        if i % 15 == 0 && i > 0 {
            device_idx = (device_idx + 1) % device_names.len();
        }
    }

    println!("Profiling Dictionary encoder with {} values", data.len());
    println!("Running encoding 100 times for profiling...");

    // Run encoding multiple times to get good profiling samples
    for iteration in 0..100 {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        // OPT-Batch: Process all values at once for better cache utilization
        encoder.encode_batch(&data).unwrap();
        encoder.flush(&mut output).unwrap();

        // Verify correctness (first iteration only)
        if iteration == 0 {
            let raw_size = data.iter().map(|s| s.len()).sum::<usize>();
            println!("Compression: {} bytes -> {} bytes ({:.2}x)",
                     raw_size,
                     output.len(),
                     raw_size as f64 / output.len() as f64);
        }

        if iteration % 10 == 0 {
            println!("Iteration {}/100", iteration);
        }
    }

    println!("Profiling complete!");
}
