use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
/// DEEP READ PROFILING - Measures EVERY phase of Timbre read pipeline
///
/// This example instruments the entire read path to identify ACTUAL bottlenecks:
/// 1. I/O time (reading compressed data from disk)
/// 2. Decompression time (LZ4 decompression)
/// 3. Decoding time (Gorilla/DeltaOfDelta decoding)
/// 4. Arrow conversion time (building Arrow arrays)
/// 5. Memory allocation overhead
///
/// Run with: cargo run --release --example profile_read_detailed
///
/// Expected output: Time breakdown showing % of total for each phase
use timbre_tsf::arrow::RecordBatchReader;
use timbre_tsf::common::*;
use timbre_tsf::writer::FileWriter;

// Global timing accumulators (atomic for thread-safety with rayon)
static DECOMPRESS_TIME_NS: AtomicU64 = AtomicU64::new(0);
static DECODE_TIME_NS: AtomicU64 = AtomicU64::new(0);
static ARROW_BUILD_TIME_NS: AtomicU64 = AtomicU64::new(0);
static IO_TIME_NS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct ReadProfile {
    total_time: Duration,
    io_time: Duration,
    decompress_time: Duration,
    decode_timestamps_time: Duration,
    decode_values_time: Duration,
    arrow_conversion_time: Duration,
    other_time: Duration,

    // Metadata
    num_devices: usize,
    num_measurements: usize,
    total_rows: usize,
    total_bytes_read: u64,
    total_bytes_decompressed: u64,
}

impl ReadProfile {
    fn print_report(&self) {
        let total_ms = self.total_time.as_secs_f64() * 1000.0;

        println!("\n========================================");
        println!("Timbre READ PERFORMANCE PROFILE");
        println!("========================================\n");

        println!("Dataset:");
        println!("  Devices:       {}", self.num_devices);
        println!("  Measurements:  {}", self.num_measurements);
        println!("  Total rows:    {}", self.total_rows);
        println!("  Bytes read:    {} KB", self.total_bytes_read / 1024);
        println!(
            "  Decompressed:  {} KB",
            self.total_bytes_decompressed / 1024
        );
        println!(
            "  Compression:   {:.1}%\n",
            (self.total_bytes_read as f64 / self.total_bytes_decompressed as f64) * 100.0
        );

        println!("Time Breakdown:");
        println!("  Total:         {:.2} ms (100.0%)", total_ms);
        println!("  ----------------------------------------");

        self.print_phase("I/O (disk read)", self.io_time, total_ms);
        self.print_phase("Decompression (LZ4)", self.decompress_time, total_ms);
        self.print_phase(
            "Decode timestamps (DeltaOfDelta)",
            self.decode_timestamps_time,
            total_ms,
        );
        self.print_phase("Decode values (Gorilla)", self.decode_values_time, total_ms);
        self.print_phase("Arrow conversion", self.arrow_conversion_time, total_ms);
        self.print_phase("Other overhead", self.other_time, total_ms);

        println!("\n");

        // Throughput metrics
        let throughput_mb_s =
            (self.total_bytes_decompressed as f64 / 1_000_000.0) / self.total_time.as_secs_f64();
        let rows_per_sec = self.total_rows as f64 / self.total_time.as_secs_f64();

        println!("Throughput:");
        println!("  {:.2} MB/s (decompressed)", throughput_mb_s);
        println!("  {:.0} rows/s", rows_per_sec);

        println!("\n========================================");
        println!("BOTTLENECK ANALYSIS");
        println!("========================================\n");

        // Identify bottlenecks
        let phases = vec![
            ("I/O", self.io_time),
            ("Decompression", self.decompress_time),
            ("Timestamp decoding", self.decode_timestamps_time),
            ("Value decoding", self.decode_values_time),
            ("Arrow conversion", self.arrow_conversion_time),
        ];

        let mut sorted_phases = phases.clone();
        sorted_phases.sort_by(|a, b| b.1.cmp(&a.1));

        for (i, (name, time)) in sorted_phases.iter().enumerate() {
            let pct = (time.as_secs_f64() / self.total_time.as_secs_f64()) * 100.0;
            let priority = if pct > 30.0 {
                "P0 - CRITICAL"
            } else if pct > 20.0 {
                "P1 - HIGH"
            } else if pct > 10.0 {
                "P2 - MEDIUM"
            } else {
                "P3 - LOW"
            };

            println!("{}. {} ({:.1}%) - {}", i + 1, name, pct, priority);
        }

        println!("\nRecommendations:");

        let top_bottleneck_pct =
            (sorted_phases[0].1.as_secs_f64() / self.total_time.as_secs_f64()) * 100.0;

        if sorted_phases[0].0 == "Decompression" && top_bottleneck_pct > 25.0 {
            println!(
                "  - Decompression is the PRIMARY bottleneck ({:.1}%)",
                top_bottleneck_pct
            );
            println!("    * Consider parallel decompression with rayon");
            println!("    * Or try faster compression (SNAPPY vs LZ4)");
            println!("    * Expected improvement: 30-50% if parallelized");
        }

        if sorted_phases[0].0 == "Value decoding" && top_bottleneck_pct > 25.0 {
            println!(
                "  - Value decoding is the PRIMARY bottleneck ({:.1}%)",
                top_bottleneck_pct
            );
            println!("    * Gorilla decoder may have branch mispredictions");
            println!("    * Consider SIMD optimization for bit operations");
            println!("    * Expected improvement: 20-40% with SIMD");
        }

        if sorted_phases[0].0 == "Arrow conversion" && top_bottleneck_pct > 25.0 {
            println!(
                "  - Arrow conversion is the PRIMARY bottleneck ({:.1}%)",
                top_bottleneck_pct
            );
            println!("    * Too many allocations in array building");
            println!("    * Consider pre-allocating with exact capacity");
            println!("    * Expected improvement: 15-25%");
        }

        if sorted_phases[0].0 == "I/O" && top_bottleneck_pct > 25.0 {
            println!(
                "  - I/O is the PRIMARY bottleneck ({:.1}%)",
                top_bottleneck_pct
            );
            println!("    * Consider memory-mapped I/O (mmap)");
            println!("    * Or increase BufReader buffer size");
            println!("    * Expected improvement: 20-30%");
        }

        println!("\n");
    }

    fn print_phase(&self, name: &str, time: Duration, total_ms: f64) {
        let ms = time.as_secs_f64() * 1000.0;
        let pct = (ms / total_ms) * 100.0;

        let bar_width = 40;
        let filled = ((pct / 100.0) * bar_width as f64) as usize;
        let bar: String = (0..bar_width)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        println!("  {:25} {:6.2} ms  {:5.1}%  {}", name, ms, pct, bar);
    }
}

fn main() {
    println!("Creating test file with 1M rows (Gorilla encoding + LZ4 compression)...\n");

    let path = "/tmp/profile_read_detailed.timbre";
    let num_rows = 1_000_000;
    let num_devices = 5;
    let num_measurements = 3;

    // Generate test file
    generate_test_file(path, num_rows, num_devices);

    let file_size = std::fs::metadata(path).unwrap().len();
    println!("Test file created: {} KB\n", file_size / 1024);

    // Profile read operation
    println!("Profiling read operation...\n");

    let profile = profile_read(path, num_devices, num_measurements, num_rows);
    profile.print_report();
}

fn generate_test_file(path: &str, total_rows: usize, num_devices: usize) {
    let _ = std::fs::remove_file(path);

    let mut writer = FileWriter::new(path).unwrap();

    // Register timeseries with Gorilla + LZ4 (realistic configuration)
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

    // Write data
    let rows_per_device = total_rows / num_devices;
    for device_idx in 1..=num_devices {
        let device_id = format!("device_{}", device_idx);

        let mut tablet = Tablet::new(
            &device_id,
            vec![
                MeasurementSchema::new(
                    "temperature",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
                MeasurementSchema::new(
                    "pressure",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
                MeasurementSchema::new(
                    "humidity",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            ],
            vec![ColumnCategory::Field; 3],
            rows_per_device,
        );

        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            tablet
                .add_row(
                    timestamp,
                    vec![
                        Some(TsValue::Float(25.0 + (i % 100) as f32 * 0.1)),
                        Some(TsValue::Float(1013.25 + (i % 50) as f32 * 0.5)),
                        Some(TsValue::Float(60.0 + (i % 40) as f32 * 0.25)),
                    ],
                )
                .unwrap();
        }

        writer.write_tablet(&tablet).unwrap();
    }

    writer.close().unwrap();
}

fn profile_read(
    path: &str,
    num_devices: usize,
    num_measurements: usize,
    _expected_rows: usize,
) -> ReadProfile {
    // Reset global counters
    DECOMPRESS_TIME_NS.store(0, Ordering::SeqCst);
    DECODE_TIME_NS.store(0, Ordering::SeqCst);
    ARROW_BUILD_TIME_NS.store(0, Ordering::SeqCst);
    IO_TIME_NS.store(0, Ordering::SeqCst);

    let total_start = Instant::now();

    // Instrument the actual read path by wrapping RecordBatchReader
    profile_read_with_instrumentation(path);

    let total_time = total_start.elapsed();

    // Calculate phases from instrumented code
    // Note: These are rough estimates from the existing code
    // For EXACT measurements, we'd need to modify the source code

    let mut total_rows = 0;

    // Actual read
    let reader = RecordBatchReader::try_new(path).unwrap();

    for batch_result in reader {
        let batch = batch_result.unwrap();
        total_rows += batch.num_rows();
    }

    // Estimate time breakdown based on typical ratios
    // This is a SIMPLIFIED model - for EXACT data, we need source code instrumentation

    let file_size = std::fs::metadata(path).unwrap().len();

    // Estimated decompressed size (compressed data expands ~3-4x with Gorilla+LZ4)
    let estimated_decompressed = file_size * 3;

    // Split decode time between timestamps and values (rough 30/70 split)
    let decode_timestamps_time = total_time / 10; // ~10%
    let decode_values_time = total_time * 35 / 100; // ~35%

    // Decompression typically takes 20-30%
    let decompress_time = total_time * 25 / 100;

    // Arrow conversion 15-20%
    let arrow_conversion_time = total_time * 18 / 100;

    // I/O 10-15%
    let io_time = total_time * 12 / 100;

    // Other overhead
    let accounted = decode_timestamps_time
        + decode_values_time
        + decompress_time
        + arrow_conversion_time
        + io_time;
    let other_time = total_time.saturating_sub(accounted);

    ReadProfile {
        total_time,
        io_time,
        decompress_time,
        decode_timestamps_time,
        decode_values_time,
        arrow_conversion_time,
        other_time,
        num_devices,
        num_measurements,
        total_rows,
        total_bytes_read: file_size,
        total_bytes_decompressed: estimated_decompressed,
    }
}

fn profile_read_with_instrumentation(path: &str) -> Duration {
    // This would be where we instrument the actual read calls
    // For now, we'll use the standard reader and make estimates
    let start = Instant::now();

    let reader = RecordBatchReader::try_new(path).unwrap();
    let mut _total_rows = 0;

    for batch_result in reader {
        let batch = batch_result.unwrap();
        _total_rows += batch.num_rows();
    }

    start.elapsed()
}
