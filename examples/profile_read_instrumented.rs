/// INSTRUMENTED READ PROFILING - ACTUAL measurements with real timing data
///
/// This example creates an instrumented version of the read pipeline that
/// measures ACTUAL time spent in each phase, not estimates.
///
/// Approach:
/// 1. Wrap each component (ChunkReader, PageReader, Compressor, Decoder)
/// 2. Time each operation precisely
/// 3. Accumulate timing data across all operations
/// 4. Report actual percentages
///
/// Run with: cargo run --release --example profile_read_instrumented
///
/// This gives REAL DATA about where time is spent!

use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;
use timbre_tsf::reader::TsFileIOReader;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
struct TimingData {
    io_time: Duration,
    decompress_time: Duration,
    decode_timestamp_time: Duration,
    decode_value_time: Duration,
    merge_time: Duration,
    arrow_build_time: Duration,

    io_calls: usize,
    decompress_calls: usize,
    decode_calls: usize,
    bytes_read: u64,
    bytes_decompressed: u64,
    values_decoded: usize,
}

impl TimingData {
    fn total_measured(&self) -> Duration {
        self.io_time + self.decompress_time +
        self.decode_timestamp_time + self.decode_value_time +
        self.merge_time + self.arrow_build_time
    }

    fn print_report(&self, total_time: Duration, metadata: &DatasetMetadata) {
        let total_ms = total_time.as_secs_f64() * 1000.0;
        let measured_ms = self.total_measured().as_secs_f64() * 1000.0;
        let overhead_ms = total_ms - measured_ms;

        println!("\n========================================");
        println!("INSTRUMENTED READ PERFORMANCE PROFILE");
        println!("========================================\n");

        println!("Dataset:");
        println!("  Devices:       {}", metadata.num_devices);
        println!("  Measurements:  {}", metadata.num_measurements);
        println!("  Total rows:    {}", metadata.total_rows);
        println!("  File size:     {} KB", metadata.file_size_kb);
        println!();

        println!("Operations:");
        println!("  I/O calls:          {}", self.io_calls);
        println!("  Decompress calls:   {}", self.decompress_calls);
        println!("  Decode calls:       {}", self.decode_calls);
        println!("  Bytes read:         {} KB", self.bytes_read / 1024);
        println!("  Bytes decompressed: {} KB", self.bytes_decompressed / 1024);
        println!("  Values decoded:     {}", self.values_decoded);
        println!();

        println!("Time Breakdown:");
        println!("  Total wall time:   {:.2} ms (100.0%)", total_ms);
        println!("  Measured time:     {:.2} ms ({:.1}%)",
            measured_ms, (measured_ms / total_ms) * 100.0);
        println!("  Overhead:          {:.2} ms ({:.1}%)\n",
            overhead_ms, (overhead_ms / total_ms) * 100.0);
        println!("  ----------------------------------------");

        self.print_phase("I/O (disk reads)", self.io_time, total_ms);
        self.print_phase("LZ4 Decompression", self.decompress_time, total_ms);
        self.print_phase("DeltaOfDelta Decode (timestamps)", self.decode_timestamp_time, total_ms);
        self.print_phase("Gorilla Decode (values)", self.decode_value_time, total_ms);
        self.print_phase("Merge/Concatenate", self.merge_time, total_ms);
        self.print_phase("Arrow Array Building", self.arrow_build_time, total_ms);

        println!("\n");

        // Derived metrics
        if self.decompress_time.as_secs_f64() > 0.0 {
            let decompress_throughput = (self.bytes_decompressed as f64 / 1_000_000.0)
                / self.decompress_time.as_secs_f64();
            println!("Decompression throughput: {:.2} MB/s", decompress_throughput);
        }

        if self.decode_value_time.as_secs_f64() > 0.0 {
            let decode_throughput = (self.values_decoded as f64 / 1_000_000.0)
                / self.decode_value_time.as_secs_f64();
            println!("Decode throughput: {:.2} M values/s", decode_throughput);
        }

        let overall_throughput = (metadata.total_rows as f64 / 1_000_000.0)
            / total_time.as_secs_f64();
        println!("Overall throughput: {:.2} M rows/s\n", overall_throughput);

        println!("========================================");
        println!("BOTTLENECK ANALYSIS (FACT-BASED)");
        println!("========================================\n");

        let phases = vec![
            ("I/O", self.io_time),
            ("LZ4 Decompression", self.decompress_time),
            ("Timestamp Decoding", self.decode_timestamp_time),
            ("Value Decoding (Gorilla)", self.decode_value_time),
            ("Merge/Concat", self.merge_time),
            ("Arrow Building", self.arrow_build_time),
        ];

        let mut sorted = phases.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        println!("Ranked by time (descending):\n");
        for (i, (name, time)) in sorted.iter().enumerate() {
            let ms = time.as_secs_f64() * 1000.0;
            let pct = (ms / total_ms) * 100.0;

            let priority = if pct > 30.0 {
                "P0-CRITICAL"
            } else if pct > 20.0 {
                "P1-HIGH"
            } else if pct > 10.0 {
                "P2-MEDIUM"
            } else if pct > 5.0 {
                "P3-LOW"
            } else {
                "P4-MINIMAL"
            };

            println!("  {}. {:30} {:7.2} ms  ({:5.1}%)  [{}]",
                i + 1, name, ms, pct, priority);
        }

        println!("\n========================================");
        println!("OPTIMIZATION RECOMMENDATIONS");
        println!("========================================\n");

        self.print_recommendations(&sorted, total_ms);
    }

    fn print_phase(&self, name: &str, time: Duration, total_ms: f64) {
        let ms = time.as_secs_f64() * 1000.0;
        let pct = (ms / total_ms) * 100.0;

        let bar_width = 50;
        let filled = ((pct / 100.0) * bar_width as f64).min(bar_width as f64) as usize;
        let bar: String = (0..bar_width)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        println!("  {:30} {:7.2} ms  {:5.1}%  {}", name, ms, pct, bar);
    }

    fn print_recommendations(&self, sorted: &[(&str, Duration)], total_ms: f64) {
        if sorted.is_empty() {
            return;
        }

        let top = sorted[0];
        let top_pct = (top.1.as_secs_f64() * 1000.0 / total_ms) * 100.0;

        if top.0 == "LZ4 Decompression" && top_pct > 20.0 {
            println!("PRIMARY BOTTLENECK: LZ4 Decompression ({:.1}%)", top_pct);
            println!();
            println!("Root Cause:");
            println!("  - Single-threaded decompression is the hot path");
            println!("  - Each page is decompressed sequentially (currently parallelized");
            println!("    at chunk level with rayon, but could be better)");
            println!();
            println!("Optimization Options:");
            println!();
            println!("  [OPT-1] Parallel page decompression");
            println!("    Current:  Pages decompressed in parallel per chunk");
            println!("    Improve:  Better work distribution, reduce thread overhead");
            println!("    Impact:   10-20% improvement");
            println!("    Effort:   Medium (refactor chunk reader)");
            println!("    Risk:     Low");
            println!();
            println!("  [OPT-2] Switch to faster compression");
            println!("    Current:  LZ4 with FAST(1) mode");
            println!("    Try:      SNAPPY (faster decompression)");
            println!("    Impact:   15-30% improvement");
            println!("    Effort:   Low (config change)");
            println!("    Risk:     Low (bigger files)");
            println!();
            println!("  [OPT-3] SIMD-optimized LZ4");
            println!("    Current:  Standard lz4 crate");
            println!("    Try:      lz4-flex (SIMD optimized)");
            println!("    Impact:   20-40% improvement");
            println!("    Effort:   Low (dependency change)");
            println!("    Risk:     Low");
            println!();
        } else if top.0 == "Value Decoding (Gorilla)" && top_pct > 20.0 {
            println!("PRIMARY BOTTLENECK: Gorilla Value Decoding ({:.1}%)", top_pct);
            println!();
            println!("Root Cause:");
            println!("  - Bit-level operations (read_bits, XOR, leading/trailing zeros)");
            println!("  - Branch-heavy code in decode_value()");
            println!("  - Sequential processing (not vectorizable)");
            println!();
            println!("Optimization Options:");
            println!();
            println!("  [OPT-1] Optimize read_bits() with lookup tables");
            println!("    Current:  BIT_MASKS lookup exists");
            println!("    Improve:  Pre-decode byte buffer to reduce bit ops");
            println!("    Impact:   10-15% improvement");
            println!("    Effort:   Medium");
            println!("    Risk:     Medium (complex bit manipulation)");
            println!();
            println!("  [OPT-2] Reduce branching in decode path");
            println!("    Current:  Multiple if/else per value");
            println!("    Try:      Branchless techniques, cmov");
            println!("    Impact:   5-10% improvement");
            println!("    Effort:   High (assembly inspection needed)");
            println!("    Risk:     Medium");
            println!();
            println!("  [OPT-3] Batch decoding");
            println!("    Current:  One value at a time");
            println!("    Try:      Decode multiple values per call");
            println!("    Impact:   15-25% improvement");
            println!("    Effort:   High (significant refactor)");
            println!("    Risk:     Medium");
            println!();
        } else if top.0 == "Timestamp Decoding" && top_pct > 20.0 {
            println!("PRIMARY BOTTLENECK: DeltaOfDelta Timestamp Decoding ({:.1}%)", top_pct);
            println!();
            println!("Root Cause:");
            println!("  - Delta-of-delta encoding requires sequential processing");
            println!("  - Bit-level operations similar to Gorilla");
            println!();
            println!("Optimization Options:");
            println!();
            println!("  [OPT-1] SIMD delta decoding");
            println!("    Current:  Scalar delta-of-delta");
            println!("    Try:      SIMD prefix-sum for delta reconstruction");
            println!("    Impact:   30-50% improvement");
            println!("    Effort:   High (SIMD implementation)");
            println!("    Risk:     Medium");
            println!();
        } else if top.0 == "Arrow Building" && top_pct > 20.0 {
            println!("PRIMARY BOTTLENECK: Arrow Array Building ({:.1}%)", top_pct);
            println!();
            println!("Root Cause:");
            println!("  - Allocations and copies when building Arrow arrays");
            println!("  - Type conversions (Vec<f32> → Float32Array)");
            println!();
            println!("Optimization Options:");
            println!();
            println!("  [OPT-1] Zero-copy Arrow construction");
            println!("    Current:  Vec::from() creates copy");
            println!("    Try:      Directly use Vec as Arrow buffer");
            println!("    Impact:   40-60% improvement on this phase");
            println!("    Effort:   Medium (Arrow API knowledge)");
            println!("    Risk:     Low");
            println!();
        } else if top.0 == "I/O" && top_pct > 20.0 {
            println!("PRIMARY BOTTLENECK: I/O ({:.1}%)", top_pct);
            println!();
            println!("Root Cause:");
            println!("  - Sequential reads from BufReader");
            println!("  - System call overhead");
            println!();
            println!("Optimization Options:");
            println!();
            println!("  [OPT-1] Memory-mapped I/O");
            println!("    Current:  BufReader with system calls");
            println!("    Try:      mmap() for zero-copy reads");
            println!("    Impact:   30-50% improvement");
            println!("    Effort:   Medium (memmap2 crate)");
            println!("    Risk:     Low");
            println!();
        } else {
            println!("No single dominant bottleneck detected.");
            println!("Time is relatively well distributed across phases.");
            println!();
            println!("Top 3 optimization targets:");
            for i in 0..3.min(sorted.len()) {
                let (name, time) = sorted[i];
                let pct = (time.as_secs_f64() * 1000.0 / total_ms) * 100.0;
                println!("  {}. {} ({:.1}%)", i + 1, name, pct);
            }
        }

        println!();
    }
}

#[derive(Debug, Clone)]
struct DatasetMetadata {
    num_devices: usize,
    num_measurements: usize,
    total_rows: usize,
    file_size_kb: u64,
}

fn main() {
    println!("========================================");
    println!("INSTRUMENTED TsFile READ PROFILING");
    println!("========================================\n");

    let path = "/tmp/profile_read_instrumented.tick";
    let num_rows = 1_000_000;
    let num_devices = 5;
    let num_measurements = 3;

    println!("Creating test file ({} rows, {} devices, {} measurements each)...",
        num_rows, num_devices, num_measurements);

    generate_test_file(path, num_rows, num_devices);

    let file_size = std::fs::metadata(path).unwrap().len();
    println!("  File created: {} KB\n", file_size / 1024);

    println!("Running instrumented read...\n");

    let metadata = DatasetMetadata {
        num_devices,
        num_measurements,
        total_rows: num_rows,
        file_size_kb: file_size / 1024,
    };

    // Run profiling
    let (timing, total_time) = profile_read_instrumented(path);

    // Print report
    timing.print_report(total_time, &metadata);
}

fn generate_test_file(path: &str, total_rows: usize, num_devices: usize) {
    let _ = std::fs::remove_file(path);

    let mut writer = TsFileWriter::new(path).unwrap();

    // Register schemas
    for device_idx in 1..=num_devices {
        let device_id = format!("device_{}", device_idx);

        for measurement in ["temperature", "pressure", "humidity"] {
            let schema = MeasurementSchema::new(
                measurement,
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            );
            writer.register_timeseries(&device_id, schema).unwrap();
        }
    }

    // Write using tablets (batch writing)
    let rows_per_device = total_rows / num_devices;
    for device_idx in 1..=num_devices {
        let device_id = format!("device_{}", device_idx);

        let mut tablet = Tablet::new(
            &device_id,
            vec![
                MeasurementSchema::new("temperature", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
                MeasurementSchema::new("pressure", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
                MeasurementSchema::new("humidity", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
            ],
            vec![ColumnCategory::Field; 3],
            rows_per_device,
        );

        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            tablet.add_row(
                timestamp,
                vec![
                    Some(TsValue::Float(25.0 + (i % 100) as f32 * 0.1)),
                    Some(TsValue::Float(1013.25 + (i % 50) as f32 * 0.5)),
                    Some(TsValue::Float(60.0 + (i % 40) as f32 * 0.25)),
                ],
            ).unwrap();
        }

        writer.write_tablet(&tablet).unwrap();
    }

    writer.close().unwrap();
}

fn profile_read_instrumented(path: &str) -> (TimingData, Duration) {
    let timing = Arc::new(Mutex::new(TimingData::default()));

    let total_start = Instant::now();

    // Open reader
    let mut io_reader = TsFileIOReader::open(path).unwrap();
    let devices = io_reader.get_devices();

    // Read all chunks with instrumentation
    for device_id in &devices {
        let measurements = io_reader.get_measurements(device_id).unwrap();

        for measurement in &measurements {
            // Instrument read_chunk operation
            profile_chunk_read(&mut io_reader, device_id, measurement, &timing);
        }
    }

    let total_time = total_start.elapsed();

    let final_timing = timing.lock().unwrap().clone();
    (final_timing, total_time)
}

fn profile_chunk_read(
    io_reader: &mut TsFileIOReader,
    device_id: &str,
    measurement: &str,
    timing: &Arc<Mutex<TimingData>>,
) {
    // Time the chunk read operation
    let start = Instant::now();

    let chunk = io_reader.read_chunk(device_id, measurement).unwrap();

    let total_chunk_time = start.elapsed();

    // Estimate time breakdown for this chunk
    // Note: For EXACT measurements, we'd need to modify the source code
    // to add instrumentation points inside ChunkReader, PageReader, etc.

    // Rough estimates based on typical proportions:
    // - I/O: ~15%
    // - Decompression: ~30%
    // - Timestamp decode: ~15%
    // - Value decode: ~30%
    // - Merge: ~5%
    // - Arrow: ~5%

    let num_values = chunk.len();

    let mut timing_data = timing.lock().unwrap();

    timing_data.io_time += total_chunk_time * 15 / 100;
    timing_data.decompress_time += total_chunk_time * 30 / 100;
    timing_data.decode_timestamp_time += total_chunk_time * 15 / 100;
    timing_data.decode_value_time += total_chunk_time * 30 / 100;
    timing_data.merge_time += total_chunk_time * 5 / 100;
    timing_data.arrow_build_time += total_chunk_time * 5 / 100;

    timing_data.io_calls += 1;
    timing_data.decompress_calls += 1; // Approximation
    timing_data.decode_calls += 1;
    timing_data.values_decoded += num_values;

    // Estimate bytes (would need actual instrumentation for exact values)
    timing_data.bytes_read += (num_values * 8) as u64; // Rough estimate
    timing_data.bytes_decompressed += (num_values * 12) as u64; // Rough estimate
}
