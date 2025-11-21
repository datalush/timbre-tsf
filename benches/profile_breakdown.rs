// Detailed breakdown profiling of write pipeline
// Run: cargo run --release --bench profile_breakdown

use std::time::Instant;
use timbre_tsf::common::{CompressionType, TSDataType, TSEncoding};
use timbre_tsf::encoding::{create_encoder, EncoderImpl};
use timbre_tsf::compress::create_compressor;

macro_rules! measure {
    ($label:expr, $iterations:expr, $code:block) => {{
        let start = Instant::now();
        for _ in 0..$iterations {
            $code
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed.as_secs_f64() / $iterations as f64;
        let throughput_mb = (4000.0 * 4.0) / per_iter / 1024.0 / 1024.0; // 4000 f32 values = 16KB
        println!(
            "{:40} | {:8.3}ms | {:8.2} MB/s",
            $label,
            per_iter * 1000.0,
            throughput_mb
        );
    }};
}

fn main() {
    println!("\n=== Encoding Pipeline Breakdown ===\n");
    println!("{:40} | {:>8} | {:>11}", "Component", "Time", "Throughput");
    println!("{:-<40}-+-{:-<8}-+-{:-<11}", "", "", "");

    // Prepare test data: 4000 float values (typical mini-block size)
    let values: Vec<f32> = (0..4000).map(|i| 20.0 + (i as f32) * 0.01).collect();
    let timestamps: Vec<i64> = (0..4000).map(|i| 1000000 + i * 1000).collect();

    // 1. Chimp128 encoding
    measure!("Chimp128 encode (4000 values)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&values, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // 2. Gorilla encoding
    measure!("Gorilla encode (4000 values)", 1000, {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&values, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // 3. DeltaOfDelta encoding (timestamps)
    measure!("DeltaOfDelta encode (4000 values)", 1000, {
        let mut encoder = create_encoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
        let mut buffer = Vec::new();
        encoder.encode_i64_batch(&timestamps, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();
    });

    // Prepare encoded data for compression tests
    let mut chimp_encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    let mut encoded_data = Vec::new();
    chimp_encoder.encode_f32_batch(&values, &mut encoded_data).unwrap();
    chimp_encoder.flush(&mut encoded_data).unwrap();
    let encoded_size = encoded_data.len();

    println!("\n{:40} | Encoded: {} bytes", "Chimp128 output size", encoded_size);

    // 4. Snappy compression
    measure!("Snappy compress (encoded data)", 1000, {
        let mut compressor = create_compressor(CompressionType::Snappy);
        let _ = compressor.compress(&encoded_data).unwrap();
    });

    // 5. LZ4 compression
    measure!("LZ4 compress (encoded data)", 1000, {
        let mut compressor = create_compressor(CompressionType::Lz4);
        let _ = compressor.compress(&encoded_data).unwrap();
    });

    // 6. Zstd compression
    measure!("Zstd compress (encoded data)", 1000, {
        let mut compressor = create_compressor(CompressionType::Zstd);
        let _ = compressor.compress(&encoded_data).unwrap();
    });

    // 7. Combined: encode + compress (realistic mini-block)
    println!("\n{:-<40}-+-{:-<8}-+-{:-<11}", "", "", "");
    measure!("FULL: Chimp128 + Snappy", 1000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        let mut buffer = Vec::new();
        encoder.encode_f32_batch(&values, &mut buffer).unwrap();
        encoder.flush(&mut buffer).unwrap();

        let mut compressor = create_compressor(CompressionType::Snappy);
        let _ = compressor.compress(&buffer).unwrap();
    });

    // 8. Allocation overhead
    measure!("Vec::new() + reserve (allocation)", 100000, {
        let mut v: Vec<u8> = Vec::new();
        v.reserve(encoded_size);
    });

    measure!("Vec::with_capacity (allocation)", 100000, {
        let _v: Vec<u8> = Vec::with_capacity(encoded_size);
    });

    // 9. Encoder creation overhead
    measure!("create_encoder (allocation)", 10000, {
        let _ = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
    });

    measure!("encoder.reset() (reuse)", 100000, {
        let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Float);
        encoder.reset();
    });

    println!("\n=== Analysis ===");
    println!("For 2M rows (500 mini-blocks @ 4000 values each):");
    println!("  - Chimp128 encoding: ~500 iterations");
    println!("  - Snappy compression: ~500 iterations (time + value)");
    println!("  - Vec allocations: ~1000 (time_buffer + value_buffer per mini-block)");
    println!("  - Encoder creations: 0 (reused)");
    println!();
}
