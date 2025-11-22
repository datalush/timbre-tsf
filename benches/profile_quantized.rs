// Quantized encoding performance benchmark
// Run: cargo build --release --bench profile_quantized && perf record -F 99 -g ./target/release/deps/profile_quantized-*

use std::time::Instant;
use timbre_tsf::encoding::quantized::{detect_quantization, QuantizedEncoder};

macro_rules! measure {
    ($label:expr, $iterations:expr, $data:expr, $code:block) => {{
        let start = Instant::now();
        for _ in 0..$iterations {
            $code
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed.as_secs_f64() / $iterations as f64;
        let data_size = $data.len() * 8; // f64 = 8 bytes
        let throughput_mb = (data_size as f64) / per_iter / 1024.0 / 1024.0;
        println!(
            "{:50} | {:8.3}ms | {:8.2} MB/s",
            $label,
            per_iter * 1000.0,
            throughput_mb
        );
    }};
}

fn main() {
    println!("\n=== Quantized Encoding Performance ===\n");
    println!("{:50} | {:>8} | {:>11}", "Test Case", "Time", "Throughput");
    println!("{:-<50}-+-{:-<8}-+-{:-<11}", "", "", "");

    // Pattern 1: IoT temperature sensor with 0.1°C resolution (highly compressible)
    let mut temp_data = Vec::new();
    let mut temp: f64 = 20.0;
    for i in 0..10000 {
        temp_data.push(temp);
        // 85% stay same, 15% change by ±0.1°C
        if i % 7 == 0 {
            temp += if i % 2 == 0 { 0.1 } else { -0.1 };
            temp = temp.clamp(19.0, 21.0);
        }
    }

    let (min, step) = detect_quantization(&temp_data).expect("Should detect");

    measure!("Encode: IoT temp sensor (0.1°C steps, mostly stable)", 1000, temp_data, {
        let mut encoder = QuantizedEncoder::new(min, step);
        let _encoded = encoder.encode(&temp_data).unwrap();
    });

    // Prepare for decode benchmark
    let mut encoder = QuantizedEncoder::new(min, step);
    let encoded_temp = encoder.encode(&temp_data).unwrap();

    measure!("Decode: IoT temp sensor (0.1°C steps, mostly stable)", 1000, temp_data, {
        let _decoded = encoder.decode(&encoded_temp).unwrap();
    });

    println!("\nCompression: {} bytes -> {} bytes ({:.2}x)",
        temp_data.len() * 8,
        encoded_temp.len(),
        (temp_data.len() * 8) as f64 / encoded_temp.len() as f64
    );

    // Pattern 2: Frequently changing quantized data (0.1 steps, random walk)
    let mut random_walk = Vec::new();
    let mut value: f64 = 20.0;
    for i in 0..10000 {
        random_walk.push(value);
        // Random walk with 0.1 steps
        value += if i % 3 == 0 { 0.1 } else if i % 3 == 1 { -0.1 } else { 0.0 };
        value = value.clamp(15.0, 25.0);
    }

    let (min2, step2) = detect_quantization(&random_walk).expect("Should detect");

    measure!("Encode: Random walk (0.1 steps, frequent changes)", 1000, random_walk, {
        let mut encoder = QuantizedEncoder::new(min2, step2);
        let _encoded = encoder.encode(&random_walk).unwrap();
    });

    let mut encoder2 = QuantizedEncoder::new(min2, step2);
    let encoded_walk = encoder2.encode(&random_walk).unwrap();

    measure!("Decode: Random walk (0.1 steps, frequent changes)", 1000, random_walk, {
        let _decoded = encoder2.decode(&encoded_walk).unwrap();
    });

    println!("\nCompression: {} bytes -> {} bytes ({:.2}x)",
        random_walk.len() * 8,
        encoded_walk.len(),
        (random_walk.len() * 8) as f64 / encoded_walk.len() as f64
    );

    // Pattern 3: Small quantization steps (0.01 precision)
    let fine_grained: Vec<f64> = (0..10000).map(|i| 20.0 + (i as f64) * 0.01).collect();
    let (min3, step3) = detect_quantization(&fine_grained).expect("Should detect");

    measure!("Encode: Fine-grained (0.01 steps, linear)", 1000, fine_grained, {
        let mut encoder = QuantizedEncoder::new(min3, step3);
        let _encoded = encoder.encode(&fine_grained).unwrap();
    });

    let mut encoder3 = QuantizedEncoder::new(min3, step3);
    let encoded_fine = encoder3.encode(&fine_grained).unwrap();

    measure!("Decode: Fine-grained (0.01 steps, linear)", 1000, fine_grained, {
        let _decoded = encoder3.decode(&encoded_fine).unwrap();
    });

    println!("\nCompression: {} bytes -> {} bytes ({:.2}x)",
        fine_grained.len() * 8,
        encoded_fine.len(),
        (fine_grained.len() * 8) as f64 / encoded_fine.len() as f64
    );

    // Pattern 4: Large dataset (100K values) for throughput measurement
    let mut large_data = Vec::new();
    let mut val: f64 = 20.0;
    for i in 0..100000 {
        large_data.push(val);
        if i % 10 == 0 {
            val += if i % 2 == 0 { 0.1 } else { -0.1 };
            val = val.clamp(19.0, 21.0);
        }
    }

    let (min4, step4) = detect_quantization(&large_data).expect("Should detect");

    measure!("Encode: Large dataset (100K values)", 100, large_data, {
        let mut encoder = QuantizedEncoder::new(min4, step4);
        let _encoded = encoder.encode(&large_data).unwrap();
    });

    let mut encoder4 = QuantizedEncoder::new(min4, step4);
    let encoded_large = encoder4.encode(&large_data).unwrap();

    measure!("Decode: Large dataset (100K values)", 100, large_data, {
        let _decoded = encoder4.decode(&encoded_large).unwrap();
    });

    println!("\nCompression: {} bytes -> {} bytes ({:.2}x)",
        large_data.len() * 8,
        encoded_large.len(),
        (large_data.len() * 8) as f64 / encoded_large.len() as f64
    );

    println!("\n=== Detection Performance ===\n");

    measure!("Detect: 10K quantized values", 1000, temp_data, {
        let _ = detect_quantization(&temp_data);
    });

    measure!("Detect: 100K quantized values", 100, large_data, {
        let _ = detect_quantization(&large_data);
    });
}
