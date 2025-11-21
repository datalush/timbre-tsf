/// Profile Arrow -> TsFile conversion overhead
///
/// This example profiles the hot paths in arrow::from_arrow conversion
/// to identify bottlenecks causing the 1.9x slowdown vs Parquet.
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

use timbre_tsf::arrow::FromArrowConverter;
use timbre_tsf::common::{CompressionType, TSEncoding};

fn generate_large_batch(num_rows: usize) -> RecordBatch {
    println!("Generating {} rows of test data...", num_rows);

    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64;

    let timestamps: Vec<i64> = (0..num_rows)
        .map(|i| base_time + (i as i64 * 1000))
        .collect();
    let device_ids: Vec<&str> = (0..num_rows).map(|i| devices[i % devices.len()]).collect();
    let temperatures: Vec<f32> = (0..num_rows)
        .map(|i| 20.0 + (i as f32 * 0.001) % 10.0)
        .collect();
    let humidity: Vec<f32> = (0..num_rows)
        .map(|i| 50.0 + (i as f32 * 0.002) % 20.0)
        .collect();
    let pressure: Vec<f32> = (0..num_rows)
        .map(|i| 1013.0 + (i as f32 * 0.0005) % 50.0)
        .collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, false),
        Field::new("humidity", DataType::Float32, false),
        Field::new("pressure", DataType::Float32, false),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMillisecondArray::from(timestamps)),
            Arc::new(StringArray::from(device_ids)),
            Arc::new(Float32Array::from(temperatures)),
            Arc::new(Float32Array::from(humidity)),
            Arc::new(Float32Array::from(pressure)),
        ],
    )
    .unwrap()
}

fn profile_write_batch(batch: &RecordBatch, batch_size: usize) {
    println!(
        "\n=== Profiling write_batch with batch_size={} ===",
        batch_size
    );

    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    let mut converter = FromArrowConverter::builder(path)
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();

    let total_start = Instant::now();
    let mut batch_times = Vec::new();

    // Split into smaller batches
    let num_batches = (batch.num_rows() + batch_size - 1) / batch_size;
    println!(
        "Writing {} rows in {} batches of ~{} rows",
        batch.num_rows(),
        num_batches,
        batch_size
    );

    for batch_idx in 0..num_batches {
        let start = batch_idx * batch_size;
        let end = ((batch_idx + 1) * batch_size).min(batch.num_rows());

        let mini_batch = batch.slice(start, end - start);

        let batch_start = Instant::now();
        converter.write_batch(&mini_batch).unwrap();
        let batch_elapsed = batch_start.elapsed();

        batch_times.push(batch_elapsed);

        if batch_idx < 5 || batch_idx % 100 == 0 {
            println!(
                "  Batch {}/{}: {} rows in {:?} ({:.0} rows/sec)",
                batch_idx + 1,
                num_batches,
                mini_batch.num_rows(),
                batch_elapsed,
                mini_batch.num_rows() as f64 / batch_elapsed.as_secs_f64()
            );
        }
    }

    let write_elapsed = total_start.elapsed();
    println!("All batches written in {:?}", write_elapsed);

    let finish_start = Instant::now();
    converter.finish().unwrap();
    let finish_elapsed = finish_start.elapsed();

    let total_elapsed = total_start.elapsed();

    println!("\nTiming breakdown:");
    println!(
        "  Write batches: {:?} ({:.1}%)",
        write_elapsed,
        100.0 * write_elapsed.as_secs_f64() / total_elapsed.as_secs_f64()
    );
    println!(
        "  Finish/close:  {:?} ({:.1}%)",
        finish_elapsed,
        100.0 * finish_elapsed.as_secs_f64() / total_elapsed.as_secs_f64()
    );
    println!("  Total:         {:?}", total_elapsed);

    let file_size = std::fs::metadata(path).unwrap().len();
    let throughput_mb = (file_size as f64 / 1_000_000.0) / total_elapsed.as_secs_f64();

    println!("\nPerformance:");
    println!(
        "  Rows/sec:      {:.0}",
        batch.num_rows() as f64 / total_elapsed.as_secs_f64()
    );
    println!("  File size:     {:.2} MB", file_size as f64 / 1_000_000.0);
    println!("  Throughput:    {:.2} MB/s", throughput_mb);

    // Batch time statistics
    if batch_times.len() > 1 {
        let total_batch_time: std::time::Duration = batch_times.iter().sum();
        let avg_batch = total_batch_time / batch_times.len() as u32;
        let min_batch = batch_times.iter().min().unwrap();
        let max_batch = batch_times.iter().max().unwrap();

        println!("\nPer-batch statistics:");
        println!("  Average: {:?}", avg_batch);
        println!("  Min:     {:?}", min_batch);
        println!("  Max:     {:?}", max_batch);
        println!(
            "  Rows/sec (avg batch): {:.0}",
            batch_size as f64 / avg_batch.as_secs_f64()
        );
    }
}

fn main() {
    println!("Arrow -> TsFile Conversion Profiling");
    println!("====================================\n");

    // Test with 2M rows (matching the benchmark data size)
    let total_rows = 2_000_000;
    let batch = generate_large_batch(total_rows);

    println!("\nDataset:");
    println!("  Total rows: {}", batch.num_rows());
    println!("  Columns: {}", batch.num_columns());
    println!("  Devices: 5");
    println!("  Measurements: 3 (temperature, humidity, pressure)");

    // Test different batch sizes to find optimal chunking
    println!("\n\n### TEST 1: Large batches (10K rows) ###");
    profile_write_batch(&batch, 10_000);

    println!("\n\n### TEST 2: Medium batches (50K rows) ###");
    profile_write_batch(&batch, 50_000);

    println!("\n\n### TEST 3: Large batches (200K rows) ###");
    profile_write_batch(&batch, 200_000);

    println!("\n\n### TEST 4: Single batch (all rows) ###");
    profile_write_batch(&batch, total_rows);

    println!("\n\n====================================");
    println!("Profiling complete!");
    println!("\nNext steps:");
    println!("1. Run with `cargo flamegraph --example profile_arrow_conversion`");
    println!("2. Look for hot paths in from_arrow.rs");
    println!("3. Focus on functions taking >5% CPU time");
}
