use std::time::{Duration, Instant};
/// PRECISE PHASE MEASUREMENT using manual timing
///
/// This manually times each component operation to get EXACT measurements
/// Run with: cargo run --release --example measure_read_phases
use timbre_tsf::common::*;
use timbre_tsf::compress::Lz4Compressor;
use timbre_tsf::encoding::{Decoder, Encoder, GorillaDecoder, GorillaEncoder, create_decoder};

fn main() {
    println!("========================================");
    println!("PRECISE READ PHASE MEASUREMENT");
    println!("========================================\n");

    let num_values = 200_000; // Per measurement

    // PHASE 1: Measure Gorilla ENCODING (to create test data)
    println!(
        "Phase 1: Encoding {} float values with Gorilla...",
        num_values
    );
    let (gorilla_encoded, gorilla_encode_time) = measure_gorilla_encode(num_values);
    println!("  Encoded size: {} bytes", gorilla_encoded.len());
    println!(
        "  Encode time:  {:.2} ms\n",
        gorilla_encode_time.as_secs_f64() * 1000.0
    );

    // PHASE 2: Measure LZ4 COMPRESSION
    println!(
        "Phase 2: Compressing {} bytes with LZ4...",
        gorilla_encoded.len()
    );
    let (compressed, compress_time) = measure_lz4_compress(&gorilla_encoded);
    println!(
        "  Compressed size: {} bytes ({:.1}% of original)",
        compressed.len(),
        (compressed.len() as f64 / gorilla_encoded.len() as f64) * 100.0
    );
    println!(
        "  Compress time:   {:.2} ms\n",
        compress_time.as_secs_f64() * 1000.0
    );

    // PHASE 3: Measure LZ4 DECOMPRESSION
    println!(
        "Phase 3: Decompressing {} bytes with LZ4...",
        compressed.len()
    );
    let (decompressed, decompress_time) =
        measure_lz4_decompress(&compressed, gorilla_encoded.len());
    println!("  Decompressed size: {} bytes", decompressed.len());
    println!(
        "  Decompress time:   {:.2} ms\n",
        decompress_time.as_secs_f64() * 1000.0
    );

    // PHASE 4: Measure Gorilla DECODING
    println!("Phase 4: Decoding {} values with Gorilla...", num_values);
    let (decoded_values, gorilla_decode_time) = measure_gorilla_decode(&decompressed, num_values);
    println!("  Decoded values: {}", decoded_values.len());
    println!(
        "  Decode time:    {:.2} ms\n",
        gorilla_decode_time.as_secs_f64() * 1000.0
    );

    // PHASE 5: Measure DeltaOfDelta timestamp encoding/decoding
    println!(
        "Phase 5: Encoding {} timestamps with DeltaOfDelta...",
        num_values
    );
    let (ts_encoded, ts_encode_time) = measure_dod_encode(num_values);
    println!("  Encoded size: {} bytes", ts_encoded.len());
    println!(
        "  Encode time:  {:.2} ms\n",
        ts_encode_time.as_secs_f64() * 1000.0
    );

    println!(
        "Phase 6: Decoding {} timestamps with DeltaOfDelta...",
        num_values
    );
    let (decoded_timestamps, ts_decode_time) = measure_dod_decode(&ts_encoded, num_values);
    println!("  Decoded timestamps: {}", decoded_timestamps.len());
    println!(
        "  Decode time:        {:.2} ms\n",
        ts_decode_time.as_secs_f64() * 1000.0
    );

    // PHASE 7: Measure Arrow array building
    println!("Phase 7: Building Arrow arrays...");
    let arrow_time = measure_arrow_build(&decoded_values, &decoded_timestamps);
    println!(
        "  Arrow build time: {:.2} ms\n",
        arrow_time.as_secs_f64() * 1000.0
    );

    // SUMMARY
    println!("========================================");
    println!("PERFORMANCE BREAKDOWN (for {} values)", num_values);
    println!("========================================\n");

    // Simulate full read pipeline timing
    let total_decompress = decompress_time;
    let total_decode = gorilla_decode_time + ts_decode_time;
    let total_arrow = arrow_time;
    let total_measured = total_decompress + total_decode + total_arrow;

    println!("Measured phases:");
    print_phase("LZ4 Decompression", total_decompress, total_measured);
    print_phase("  - Gorilla decode", gorilla_decode_time, total_measured);
    print_phase("  - DeltaOfDelta decode", ts_decode_time, total_measured);
    print_phase("Arrow building", total_arrow, total_measured);
    println!("  ----------------------------------------");
    print_phase("TOTAL (measured)", total_measured, total_measured);

    println!("\n");

    // Throughput calculations
    let decompress_mb_s =
        (gorilla_encoded.len() as f64 / 1_000_000.0) / total_decompress.as_secs_f64();
    let decode_mv_s = (num_values as f64 / 1_000_000.0) / gorilla_decode_time.as_secs_f64();

    println!("Throughput:");
    println!("  Decompression: {:.1} MB/s", decompress_mb_s);
    println!("  Gorilla decode: {:.1} M values/s", decode_mv_s);
    println!(
        "  DeltaOfDelta decode: {:.1} M timestamps/s",
        (num_values as f64 / 1_000_000.0) / ts_decode_time.as_secs_f64()
    );

    println!("\n========================================");
    println!("BOTTLENECK ANALYSIS");
    println!("========================================\n");

    let phases = vec![
        ("LZ4 Decompression", total_decompress),
        ("Gorilla Decoding", gorilla_decode_time),
        ("DeltaOfDelta Decoding", ts_decode_time),
        ("Arrow Building", total_arrow),
    ];

    let mut sorted = phases.clone();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    for (i, (name, time)) in sorted.iter().enumerate() {
        let pct = (time.as_secs_f64() / total_measured.as_secs_f64()) * 100.0;
        println!(
            "  {}. {:20} {:.2} ms ({:.1}%)",
            i + 1,
            name,
            time.as_secs_f64() * 1000.0,
            pct
        );
    }

    println!("\nConclusion:");
    let top = sorted[0];
    let top_pct = (top.1.as_secs_f64() / total_measured.as_secs_f64()) * 100.0;

    if top_pct > 35.0 {
        println!(
            "  {} is the PRIMARY bottleneck ({:.1}% of time).",
            top.0, top_pct
        );
        println!("  Focus optimization efforts here for maximum impact.");
    } else {
        println!("  No single dominant bottleneck detected.");
        println!("  Optimization should target multiple areas:");
        for (name, _) in sorted.iter().take(2) {
            println!("    - {}", name);
        }
    }

    println!();
}

fn measure_gorilla_encode(num_values: usize) -> (Vec<u8>, Duration) {
    let mut encoder = GorillaEncoder::new(TSDataType::Float);
    let mut out = Vec::new();

    let start = Instant::now();

    for i in 0..num_values {
        let value = 25.0 + (i % 1000) as f32 * 0.01;
        encoder.encode_f32(value, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();

    let elapsed = start.elapsed();
    (out, elapsed)
}

fn measure_lz4_compress(data: &[u8]) -> (Vec<u8>, Duration) {
    use timbre_tsf::compress::Compressor;
    let mut compressor = Lz4Compressor;

    let start = Instant::now();
    let compressed = compressor.compress(data).unwrap();
    let elapsed = start.elapsed();

    (compressed, elapsed)
}

fn measure_lz4_decompress(data: &[u8], uncompressed_size: usize) -> (Vec<u8>, Duration) {
    use timbre_tsf::compress::Compressor;
    let mut compressor = Lz4Compressor;

    let start = Instant::now();
    let decompressed = compressor.decompress(data, uncompressed_size).unwrap();
    let elapsed = start.elapsed();

    (decompressed, elapsed)
}

fn measure_gorilla_decode(data: &[u8], num_values: usize) -> (Vec<f32>, Duration) {
    let mut decoder = GorillaDecoder::new(TSDataType::Float);
    let mut values = Vec::with_capacity(num_values);
    let mut pos = 0;

    let start = Instant::now();

    for _ in 0..num_values {
        let v = decoder.read_f32(data, &mut pos).unwrap();
        values.push(v);
    }

    let elapsed = start.elapsed();
    (values, elapsed)
}

fn measure_dod_encode(num_values: usize) -> (Vec<u8>, Duration) {
    use timbre_tsf::encoding::DeltaOfDeltaEncoder;

    let mut encoder = DeltaOfDeltaEncoder::new(TSDataType::Int64);
    let mut out = Vec::new();

    let start = Instant::now();

    for i in 0..num_values {
        let ts = 1_000_000_000 + (i as i64 * 1000);
        encoder.encode_i64(ts, &mut out).unwrap();
    }
    encoder.flush(&mut out).unwrap();

    let elapsed = start.elapsed();
    (out, elapsed)
}

fn measure_dod_decode(data: &[u8], num_values: usize) -> (Vec<i64>, Duration) {
    let mut decoder = create_decoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
    let mut timestamps = Vec::with_capacity(num_values);
    let mut pos = 0;

    let start = Instant::now();

    while decoder.has_remaining(data, pos) && timestamps.len() < num_values {
        let ts = decoder.read_i64(data, &mut pos).unwrap();
        timestamps.push(ts);
    }

    let elapsed = start.elapsed();
    (timestamps, elapsed)
}

fn measure_arrow_build(values: &[f32], timestamps: &[i64]) -> Duration {
    use arrow::array::{Float32Array, Int64Array};

    let start = Instant::now();

    let _float_array = Float32Array::from(values.to_vec());
    let _int_array = Int64Array::from(timestamps.to_vec());

    start.elapsed()
}

fn print_phase(name: &str, time: Duration, total: Duration) {
    let ms = time.as_secs_f64() * 1000.0;
    let pct = (time.as_secs_f64() / total.as_secs_f64()) * 100.0;

    let bar_width = 40;
    let filled = ((pct / 100.0) * bar_width as f64).min(bar_width as f64) as usize;
    let bar: String = (0..bar_width)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();

    println!("  {:25} {:7.2} ms  {:5.1}%  {}", name, ms, pct, bar);
}
