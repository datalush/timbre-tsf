// Full pipeline profiling: Arrow -> Tablet -> Encode -> Compress -> Write
// Run: cargo build --release --bench profile_pipeline && ./target/release/deps/profile_pipeline-*

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;
use std::time::Instant;
use timbre_tsf::arrow::ArrowToTsFileConverter;

fn main() {
    // Create 2M rows (5 devices) matching IoT benchmark
    let num_devices = 5;
    let rows_per_device = 400_000;
    let total_rows = num_devices * rows_per_device;

    println!("\n=== Full Pipeline Profiling ({} rows) ===\n", total_rows);

    // Generate data
    let start = Instant::now();
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Millisecond, None), false),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, false),
        Field::new("humidity", DataType::Float32, false),
        Field::new("pressure", DataType::Float32, false),
    ]));

    let mut timestamps = Vec::with_capacity(total_rows);
    let mut devices = Vec::with_capacity(total_rows);
    let mut temps = Vec::with_capacity(total_rows);
    let mut humidity = Vec::with_capacity(total_rows);
    let mut pressure = Vec::with_capacity(total_rows);

    for i in 0..total_rows {
        let device_id = i % num_devices;
        timestamps.push(1000000 + (i as i64) * 1000);
        devices.push(format!("device{}", device_id));
        temps.push(20.0 + (i as f32) * 0.001 + (device_id as f32) * 5.0);
        humidity.push(60.0 + (i as f32) * 0.0005);
        pressure.push(1013.25 + (i as f32) * 0.0002);
    }
    println!("  Data generation: {:.2}s", start.elapsed().as_secs_f64());

    let start = Instant::now();
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMillisecondArray::from(timestamps)),
            Arc::new(StringArray::from(devices)),
            Arc::new(Float32Array::from(temps)),
            Arc::new(Float32Array::from(humidity)),
            Arc::new(Float32Array::from(pressure)),
        ],
    )
    .unwrap();
    println!("  RecordBatch creation: {:.2}s", start.elapsed().as_secs_f64());

    // Test 1: Full write (baseline)
    println!("\n--- Full Write (baseline) ---");
    let start = Instant::now();
    let mut converter = ArrowToTsFileConverter::builder("/tmp/profile_full.tsfile")
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();
    converter.write_batch(&batch).unwrap();
    converter.finish().unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    let mb = (total_rows as f64 * 32.0) / 1024.0 / 1024.0;
    println!("  Total time: {:.3}s ({:.2} MB/s)", elapsed, mb / elapsed);

    // Test 2: Write without compression (to isolate compression overhead)
    use timbre_tsf::common::{CompressionType, TSEncoding};
    use timbre_tsf::arrow::ArrowConversionConfig;

    println!("\n--- Write with different configs ---");

    // No compression
    let config = ArrowConversionConfig {
        default_compression: CompressionType::Uncompressed,
        ..Default::default()
    };
    let start = Instant::now();
    let mut converter = ArrowToTsFileConverter::builder("/tmp/profile_nocomp.tsfile")
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .with_config(config)
        .build()
        .unwrap();
    converter.write_batch(&batch).unwrap();
    converter.finish().unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    println!("  No compression: {:.3}s ({:.2} MB/s)", elapsed, mb / elapsed);

    // Gorilla instead of Chimp128
    let config = ArrowConversionConfig {
        default_encoding_f32: TSEncoding::Gorilla,
        default_encoding_f64: TSEncoding::Gorilla,
        ..Default::default()
    };
    let start = Instant::now();
    let mut converter = ArrowToTsFileConverter::builder("/tmp/profile_gorilla.tsfile")
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .with_config(config)
        .build()
        .unwrap();
    converter.write_batch(&batch).unwrap();
    converter.finish().unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    println!("  Gorilla encoding: {:.3}s ({:.2} MB/s)", elapsed, mb / elapsed);

    println!("\n=== Summary ===");
    println!("Any difference between configs shows where the bottleneck is:");
    println!("  - Baseline vs No compression = compression overhead");
    println!("  - Baseline vs Gorilla = Chimp128 vs Gorilla overhead");
    println!();
}
