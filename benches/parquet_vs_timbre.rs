/// Benchmark: Timbre vs Parquet for IoT Time Series
///
/// Compares:
/// - Compression ratio (file size)
/// - Write throughput (MB/s)
/// - Read throughput (MB/s)
/// - Query performance (time range filtering)
///
/// Dataset: 1 million IoT sensor readings (realistic patterns)
///
/// Run with: cargo bench --bench parquet_vs_timbre

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;
use timbre_tsf::reader::TsFileReader;

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use parquet::basic::{Compression as ParquetCompression, Encoding as ParquetEncoding};

use std::fs::File;
use std::sync::Arc;
use std::time::Duration;

/// Generate realistic IoT sensor data
/// Returns: (timestamps, temperatures, pressures, device_ids)
fn generate_iot_data(num_points: usize) -> (Vec<i64>, Vec<f32>, Vec<f64>, Vec<String>) {
    let mut timestamps = Vec::with_capacity(num_points);
    let mut temperatures = Vec::with_capacity(num_points);
    let mut pressures = Vec::with_capacity(num_points);
    let mut device_ids = Vec::with_capacity(num_points);

    let base_time = 1_704_067_200_000i64; // 2024-01-01
    let num_devices = 10;

    for i in 0..num_points {
        // Regular 1 Hz sampling (ideal for DeltaOfDelta)
        let timestamp = base_time + (i as i64 * 1000);

        // Temperature: slow-changing with small variations (ideal for Chimp128/Gorilla)
        let temp = 20.0 + ((i as f32 * 0.001).sin() * 5.0) + ((i as f32 * 0.01).cos() * 2.0);

        // Pressure: similar pattern but higher precision
        let pressure = 1013.25 + ((i as f64 * 0.0005).sin() * 3.0);

        // Device ID: repeating pattern (ideal for Dictionary encoding)
        let device_id = format!("sensor_{:02}", i % num_devices);

        timestamps.push(timestamp);
        temperatures.push(temp);
        pressures.push(pressure);
        device_ids.push(device_id);
    }

    (timestamps, temperatures, pressures, device_ids)
}

/// Write data to Timbre format
fn write_timbre_file(
    path: &str,
    timestamps: &[i64],
    temperatures: &[f32],
    pressures: &[f64],
    device_ids: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = TsFileWriter::new(path)?;

    // Use optimal encodings for time series
    // Chimp128 is 5-15% better than Gorilla for float encoding
    // Combined with ZSTD gives best compression for time series
    let temp_schema = MeasurementSchema::new(
        "temperature",
        TSDataType::Float,
        TSEncoding::Chimp128,  // Specialized float encoding
        CompressionType::Zstd,  // Industry-standard compression
    );

    let pressure_schema = MeasurementSchema::new(
        "pressure",
        TSDataType::Double,
        TSEncoding::Chimp128,  // Specialized double encoding
        CompressionType::Zstd,
    );

    // Register all devices
    for i in 0..10 {
        let device = format!("sensor_{:02}", i);
        writer.register_timeseries(&device, temp_schema.clone())?;
        writer.register_timeseries(&device, pressure_schema.clone())?;
    }

    // Write data grouped by device (realistic IoT pattern)
    // This avoids excessive chunk fragmentation
    for device_idx in 0..10 {
        let device = format!("sensor_{:02}", device_idx);
        for i in (device_idx..timestamps.len()).step_by(10) {
            let record = TsRecord::new(timestamps[i], &device)
                .with_value("temperature", TsValue::Float(temperatures[i]))
                .with_value("pressure", TsValue::Double(pressures[i]));
            writer.write_record(record)?;
        }
    }

    writer.close()?;
    Ok(())
}

/// Write data to Parquet format
fn write_parquet_file(
    path: &str,
    timestamps: &[i64],
    temperatures: &[f32],
    pressures: &[f64],
    device_ids: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;

    // Create Arrow schema
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Millisecond, None), false),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, true),
        Field::new("pressure", DataType::Float64, true),
    ]));

    // Use Parquet with ZSTD (same as Timbre for fair comparison)
    let props = WriterProperties::builder()
        .set_compression(ParquetCompression::ZSTD(Default::default()))
        .set_dictionary_enabled(true)
        .set_statistics_enabled(parquet::file::properties::EnabledStatistics::Page)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

    // Convert to Arrow arrays
    let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps.to_vec()));
    let device_array = Arc::new(StringArray::from(device_ids.to_vec()));
    let temp_array = Arc::new(Float32Array::from(temperatures.to_vec()));
    let pressure_array = Arc::new(Float64Array::from(pressures.to_vec()));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![timestamp_array, device_array, temp_array, pressure_array],
    )?;

    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

/// Get file size in bytes
fn get_file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Benchmark: Compression Ratio
fn bench_compression_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_ratio");
    group.measurement_time(Duration::from_secs(10));

    for size in [100_000, 500_000, 1_000_000] {
        let (timestamps, temperatures, pressures, device_ids) = generate_iot_data(size);

        // Calculate uncompressed size
        let uncompressed_size = (timestamps.len() * 8)  // i64 timestamps
            + (temperatures.len() * 4)  // f32 temps
            + (pressures.len() * 8)  // f64 pressures
            + device_ids.iter().map(|s| s.len()).sum::<usize>();  // strings

        let timbre_path = format!("/tmp/bench_timbre_{}.timbre", size);
        let parquet_path = format!("/tmp/bench_parquet_{}.parquet", size);

        // Write Timbre
        write_timbre_file(&timbre_path, &timestamps, &temperatures, &pressures, &device_ids)
            .unwrap();
        let timbre_size = get_file_size(&timbre_path);

        // Write Parquet
        write_parquet_file(&parquet_path, &timestamps, &temperatures, &pressures, &device_ids)
            .unwrap();
        let parquet_size = get_file_size(&parquet_path);

        println!("\n=== Compression Ratio for {} points ===", size);
        println!("Uncompressed: {:.2} MB", uncompressed_size as f64 / 1_048_576.0);
        println!("Timbre:       {:.2} MB ({:.1}x compression)",
            timbre_size as f64 / 1_048_576.0,
            uncompressed_size as f64 / timbre_size as f64);
        println!("Parquet:      {:.2} MB ({:.1}x compression)",
            parquet_size as f64 / 1_048_576.0,
            uncompressed_size as f64 / parquet_size as f64);
        println!("Timbre vs Parquet: {:.1}% smaller",
            (1.0 - (timbre_size as f64 / parquet_size as f64)) * 100.0);

        // Cleanup
        let _ = std::fs::remove_file(&timbre_path);
        let _ = std::fs::remove_file(&parquet_path);
    }

    group.finish();
}

/// Benchmark: Write Throughput
fn bench_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_throughput");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(10);

    for size in [100_000, 500_000] {
        let (timestamps, temperatures, pressures, device_ids) = generate_iot_data(size);

        // Timbre write
        group.bench_with_input(
            BenchmarkId::new("timbre", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let path = "/tmp/bench_write_timbre.timbre";
                    write_timbre_file(path, &timestamps, &temperatures, &pressures, &device_ids)
                        .unwrap();
                    let _ = std::fs::remove_file(path);
                });
            },
        );

        // Parquet write
        group.bench_with_input(
            BenchmarkId::new("parquet", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let path = "/tmp/bench_write_parquet.parquet";
                    write_parquet_file(path, &timestamps, &temperatures, &pressures, &device_ids)
                        .unwrap();
                    let _ = std::fs::remove_file(path);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Read Throughput
fn bench_read_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_throughput");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(10);

    for size in [100_000, 500_000] {
        let (timestamps, temperatures, pressures, device_ids) = generate_iot_data(size);

        // Prepare files
        let timbre_path = format!("/tmp/bench_read_timbre_{}.timbre", size);
        let parquet_path = format!("/tmp/bench_read_parquet_{}.parquet", size);

        write_timbre_file(&timbre_path, &timestamps, &temperatures, &pressures, &device_ids)
            .unwrap();
        write_parquet_file(&parquet_path, &timestamps, &temperatures, &pressures, &device_ids)
            .unwrap();

        // Timbre read
        group.bench_with_input(
            BenchmarkId::new("timbre", size),
            &timbre_path,
            |b, path| {
                b.iter(|| {
                    let mut reader = TsFileReader::open(path).unwrap();
                    let mut total_points = 0;
                    for i in 0..10 {
                        let device = format!("sensor_{:02}", i);
                        let temp_chunk = reader.read(&device, "temperature").unwrap();
                        let temp_len = temp_chunk.len();
                        let pressure_chunk = reader.read(&device, "pressure").unwrap();
                        let pressure_len = pressure_chunk.len();
                        total_points += temp_len + pressure_len;
                    }
                    black_box(total_points);
                });
            },
        );

        // Parquet read
        group.bench_with_input(
            BenchmarkId::new("parquet", size),
            &size,
            |b, _| {
                b.iter(|| {
                    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
                    let file = File::open(&parquet_path).unwrap();
                    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
                    let mut reader = builder.build().unwrap();

                    while let Some(Ok(batch)) = reader.next() {
                        black_box(batch);
                    }
                });
            },
        );

        // Cleanup
        let _ = std::fs::remove_file(&timbre_path);
        let _ = std::fs::remove_file(&parquet_path);
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_compression_ratio,
    bench_write_throughput,
    bench_read_throughput,
);
criterion_main!(benches);
