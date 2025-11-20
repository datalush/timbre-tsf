use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

use timbre_tsf::arrow::ArrowToTsFileConverter;
use timbre_tsf::common::{ColumnCategory, CompressionType, MeasurementSchema, TSDataType, TSEncoding, Tablet, TsValue};
use timbre_tsf::writer::TsFileWriter;

fn generate_test_batch(num_rows: usize) -> RecordBatch {
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64;

    let timestamps: Vec<i64> = (0..num_rows).map(|i| base_time + (i as i64 * 1000)).collect();
    let device_ids: Vec<&str> = (0..num_rows).map(|i| devices[i % devices.len()]).collect();
    let temperatures: Vec<f32> = (0..num_rows).map(|i| 20.0 + (i as f32 * 0.001) % 10.0).collect();
    let humidity: Vec<f32> = (0..num_rows).map(|i| 50.0 + (i as f32 * 0.002) % 20.0).collect();
    let pressure: Vec<f32> = (0..num_rows).map(|i| 1013.0 + (i as f32 * 0.0005) % 50.0).collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Millisecond, None), false),
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

fn write_tsfile_native(batch: &RecordBatch) -> std::time::Duration {
    let temp_file = NamedTempFile::new().unwrap();
    let start = Instant::now();

    let mut writer = TsFileWriter::new(temp_file.path()).unwrap();

    // Register schemas
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    for device in &devices {
        writer.register_timeseries(*device, MeasurementSchema::new("temperature", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4)).unwrap();
        writer.register_timeseries(*device, MeasurementSchema::new("humidity", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4)).unwrap();
        writer.register_timeseries(*device, MeasurementSchema::new("pressure", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4)).unwrap();
    }

    // Extract arrays
    let timestamp_array = batch.column(0).as_any().downcast_ref::<TimestampMillisecondArray>().unwrap();
    let device_array = batch.column(1).as_any().downcast_ref::<StringArray>().unwrap();
    let temp_array = batch.column(2).as_any().downcast_ref::<Float32Array>().unwrap();
    let humidity_array = batch.column(3).as_any().downcast_ref::<Float32Array>().unwrap();
    let pressure_array = batch.column(4).as_any().downcast_ref::<Float32Array>().unwrap();

    // Group by device
    let mut rows_by_device: HashMap<String, Vec<(i64, f32, f32, f32)>> = HashMap::new();
    for i in 0..batch.num_rows() {
        let timestamp = timestamp_array.value(i);
        let device_id = device_array.value(i).to_string();
        let temperature = temp_array.value(i);
        let humidity = humidity_array.value(i);
        let pressure = pressure_array.value(i);

        rows_by_device.entry(device_id).or_insert_with(Vec::new).push((timestamp, temperature, humidity, pressure));
    }

    // Write each device using tablets
    for (device_id, rows) in rows_by_device {
        let schemas = vec![
            MeasurementSchema::new("temperature", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
            MeasurementSchema::new("humidity", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
            MeasurementSchema::new("pressure", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
        ];

        let mut tablet = Tablet::new(&device_id, schemas, vec![ColumnCategory::Field; 3], rows.len());

        for (timestamp, temperature, humidity, pressure) in rows {
            tablet.add_row(timestamp, vec![
                Some(TsValue::Float(temperature)),
                Some(TsValue::Float(humidity)),
                Some(TsValue::Float(pressure)),
            ]).unwrap();
        }

        writer.write_tablet(&tablet).unwrap();
    }

    writer.close().unwrap();
    start.elapsed()
}

fn write_tsfile_arrow(batch: &RecordBatch) -> std::time::Duration {
    let temp_file = NamedTempFile::new().unwrap();
    let start = Instant::now();

    let mut converter = ArrowToTsFileConverter::new(temp_file.path())
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();

    converter.write_batch(batch).unwrap();
    converter.finish().unwrap();

    start.elapsed()
}

fn main() {
    println!("Native vs Arrow TsFile Write Comparison\n");
    println!("========================================\n");

    let num_rows = 100_000;
    println!("Testing with {} rows\n", num_rows);

    let batch = generate_test_batch(num_rows);

    // Warmup
    println!("Warming up...");
    for _ in 0..2 {
        write_tsfile_native(&batch);
        write_tsfile_arrow(&batch);
    }

    println!("\nRunning benchmarks (5 iterations each):\n");

    // Native method
    let mut native_times = Vec::new();
    for i in 0..5 {
        let elapsed = write_tsfile_native(&batch);
        native_times.push(elapsed);
        println!("Native iteration {}: {:.2?}", i + 1, elapsed);
    }

    println!();

    // Arrow method
    let mut arrow_times = Vec::new();
    for i in 0..5 {
        let elapsed = write_tsfile_arrow(&batch);
        arrow_times.push(elapsed);
        println!("Arrow iteration {}:  {:.2?}", i + 1, elapsed);
    }

    println!("\n========================================");
    println!("RESULTS:");
    println!("========================================\n");

    let native_avg = native_times.iter().sum::<std::time::Duration>() / native_times.len() as u32;
    let native_min = native_times.iter().min().unwrap();

    let arrow_avg = arrow_times.iter().sum::<std::time::Duration>() / arrow_times.len() as u32;
    let arrow_min = arrow_times.iter().min().unwrap();

    println!("Native method:");
    println!("  Average: {:.2?}", native_avg);
    println!("  Best:    {:.2?}", native_min);
    println!("  Throughput: {:.0} rows/sec", num_rows as f64 / native_avg.as_secs_f64());

    println!("\nArrow method (OPTIMIZED):");
    println!("  Average: {:.2?}", arrow_avg);
    println!("  Best:    {:.2?}", arrow_min);
    println!("  Throughput: {:.0} rows/sec", num_rows as f64 / arrow_avg.as_secs_f64());

    println!("\nComparison:");
    let speedup = native_avg.as_secs_f64() / arrow_avg.as_secs_f64();
    if speedup > 1.0 {
        println!("  Arrow is {:.2}x FASTER than native", speedup);
    } else {
        println!("  Arrow is {:.2}x slower than native", 1.0 / speedup);
    }

    let diff_ms = (arrow_avg.as_secs_f64() - native_avg.as_secs_f64()) * 1000.0;
    if diff_ms > 0.0 {
        println!("  Arrow is {:.2}ms slower", diff_ms);
    } else {
        println!("  Arrow is {:.2}ms faster", -diff_ms);
    }
}
