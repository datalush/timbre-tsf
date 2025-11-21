// Deep encoding comparison: Chimp128 vs Gorilla with different data patterns
// Run: cargo build --release --bench profile_encoding_deep && ./target/release/deps/profile_encoding_deep-*

use std::time::Instant;
use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;

macro_rules! measure {
    ($label:expr, $iterations:expr, $code:block) => {{
        let start = Instant::now();
        for _ in 0..$iterations {
            $code
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed.as_secs_f64() / $iterations as f64;
        let throughput_mb = (4000.0 * 4.0) / per_iter / 1024.0 / 1024.0;
        println!(
            "{:50} | {:8.3}ms | {:8.2} MB/s",
            $label,
            per_iter * 1000.0,
            throughput_mb
        );
    }};
}

fn main() {
    println!("\n=== Deep Encoding Comparison ===\n");
    println!("{:50} | {:>8} | {:>11}", "Test Case", "Time", "Throughput");
    println!("{:-<50}-+-{:-<8}-+-{:-<11}", "", "", "");

    // Pattern 1: Slowly changing values (typical IoT sensor)
    let slowly_changing: Vec<f32> = (0..4000).map(|i| 20.0 + (i as f32) * 0.01).collect();

    measure!("Chimp128: slowly changing (IoT pattern)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&slowly_changing, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    measure!("Gorilla: slowly changing (IoT pattern)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&slowly_changing, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // Pattern 2: Constant value (best case for both)
    let constant: Vec<f32> = vec![25.5; 4000];

    measure!("Chimp128: constant value", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&constant, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    measure!("Gorilla: constant value", 1000, {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&constant, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // Pattern 3: Rapidly changing (worst case)
    let rapidly_changing: Vec<f32> = (0..4000).map(|i| (i as f32).sin() * 100.0).collect();

    measure!("Chimp128: rapidly changing (sin wave)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&rapidly_changing, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    measure!("Gorilla: rapidly changing (sin wave)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&rapidly_changing, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // Pattern 4: Small fluctuations (Chimp128 should excel here)
    let small_fluctuations: Vec<f32> = (0..4000)
        .map(|i| 20.0 + (i % 10) as f32 * 0.1)
        .collect();

    measure!("Chimp128: small fluctuations (±1 range)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&small_fluctuations, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    measure!("Gorilla: small fluctuations (±1 range)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&small_fluctuations, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // Check compression ratio
    println!("\n{:-<50}-+-{:-<8}-+-{:-<11}", "", "", "");
    println!("\n=== Compression Ratios (smaller is better) ===\n");

    for (name, data) in [
        ("Slowly changing", &slowly_changing),
        ("Constant", &constant),
        ("Rapidly changing", &rapidly_changing),
        ("Small fluctuations", &small_fluctuations),
    ] {
        let mut chimp = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut chimp_buf = Vec::new();
        chimp.encode_f32_batch(data, &mut chimp_buf).unwrap();
        chimp.flush(&mut chimp_buf).unwrap();

        let mut gorilla = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut gorilla_buf = Vec::new();
        gorilla.encode_f32_batch(data, &mut gorilla_buf).unwrap();
        gorilla.flush(&mut gorilla_buf).unwrap();

        let raw_size = data.len() * 4;
        println!(
            "{:25} | Chimp: {:5} bytes ({:.2}x) | Gorilla: {:5} bytes ({:.2}x)",
            name,
            chimp_buf.len(),
            raw_size as f32 / chimp_buf.len() as f32,
            gorilla_buf.len(),
            raw_size as f32 / gorilla_buf.len() as f32
        );
    }

    println!("\n=== Analysis ===");
    println!("Expected: Chimp128 should be similar speed to Gorilla (or faster)");
    println!("If Chimp128 is slower, the Case 3 branch is adding overhead.");
    println!();
}
