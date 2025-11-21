/// Profile encoding overhead vs Arrow conversion
///
/// This benchmark separates:
/// 1. Arrow → Tablet conversion (data extraction)
/// 2. Tablet → TsFile writing (encoding + compression + I/O)
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

use timbre_tsf::arrow::ArrowToTsFileConverter;
use timbre_tsf::common::{
    ColumnCategory, CompressionType, MeasurementSchema, TSDataType, TSEncoding, Tablet, TsValue,
};
use timbre_tsf::writer::TsFileWriter;

fn generate_test_batch(num_rows: usize) -> RecordBatch {
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

fn profile_arrow_conversion_only(batch: &RecordBatch) -> std::time::Duration {
    println!("\n=== Profiling Arrow → Tablet Conversion (NO writing) ===");

    let start = Instant::now();

    // Extract arrays (Arrow API)
    let timestamp_array = batch
        .column(0)
        .as_any()
        .downcast_ref::<TimestampMillisecondArray>()
        .unwrap();
    let device_array = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let temp_array = batch
        .column(2)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();
    let humidity_array = batch
        .column(3)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();
    let pressure_array = batch
        .column(4)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();

    let extraction_time = start.elapsed();
    println!("  Array extraction: {:?}", extraction_time);

    // Group by device (conversion hot path)
    use std::collections::HashMap;
    let group_start = Instant::now();

    let mut rows_by_device: HashMap<&str, Vec<(i64, f32, f32, f32)>> = HashMap::new();
    for i in 0..batch.num_rows() {
        let timestamp = timestamp_array.value(i);
        let device_id = device_array.value(i);
        let temperature = temp_array.value(i);
        let humidity = humidity_array.value(i);
        let pressure = pressure_array.value(i);

        rows_by_device
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push((timestamp, temperature, humidity, pressure));
    }

    let grouping_time = group_start.elapsed();
    println!("  Device grouping: {:?}", grouping_time);

    // Create tablets (but don't write)
    let tablet_start = Instant::now();
    let mut tablets = Vec::new();

    for (device_id, rows) in rows_by_device {
        let schemas = vec![
            MeasurementSchema::new(
                "temperature",
                TSDataType::Float,
                TSEncoding::Chimp128,
                CompressionType::Snappy,
            ),
            MeasurementSchema::new(
                "humidity",
                TSDataType::Float,
                TSEncoding::Chimp128,
                CompressionType::Snappy,
            ),
            MeasurementSchema::new(
                "pressure",
                TSDataType::Float,
                TSEncoding::Chimp128,
                CompressionType::Snappy,
            ),
        ];

        let mut tablet = Tablet::new(
            device_id,
            schemas,
            vec![ColumnCategory::Field; 3],
            rows.len(),
        );

        for (timestamp, temperature, humidity, pressure) in rows {
            tablet
                .add_row(
                    timestamp,
                    vec![
                        Some(TsValue::Float(temperature)),
                        Some(TsValue::Float(humidity)),
                        Some(TsValue::Float(pressure)),
                    ],
                )
                .unwrap();
        }

        tablets.push(tablet);
    }

    let tablet_time = tablet_start.elapsed();
    println!("  Tablet creation: {:?}", tablet_time);

    let total = start.elapsed();
    println!("  TOTAL conversion time: {:?}", total);
    println!(
        "    (extraction: {:.1}%, grouping: {:.1}%, tablets: {:.1}%)",
        100.0 * extraction_time.as_secs_f64() / total.as_secs_f64(),
        100.0 * grouping_time.as_secs_f64() / total.as_secs_f64(),
        100.0 * tablet_time.as_secs_f64() / total.as_secs_f64(),
    );

    total
}

fn profile_encoding_only(
    encoding: TSEncoding,
    compression: CompressionType,
) -> std::time::Duration {
    println!(
        "\n=== Profiling Tablet → TsFile Writing (encoding={:?}, compression={:?}) ===",
        encoding, compression
    );

    // Create pre-populated tablets
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let rows_per_device = 400_000; // 2M total / 5 devices

    let mut tablets = Vec::new();
    for device in &devices {
        let schemas = vec![
            MeasurementSchema::new("temperature", TSDataType::Float, encoding, compression),
            MeasurementSchema::new("humidity", TSDataType::Float, encoding, compression),
            MeasurementSchema::new("pressure", TSDataType::Float, encoding, compression),
        ];

        let mut tablet = Tablet::new(
            *device,
            schemas,
            vec![ColumnCategory::Field; 3],
            rows_per_device,
        );

        let base_time = 1700000000000i64;
        for i in 0..rows_per_device {
            tablet
                .add_row(
                    base_time + (i as i64 * 1000),
                    vec![
                        Some(TsValue::Float(20.0 + (i as f32 * 0.001) % 10.0)),
                        Some(TsValue::Float(50.0 + (i as f32 * 0.002) % 20.0)),
                        Some(TsValue::Float(1013.0 + (i as f32 * 0.0005) % 50.0)),
                    ],
                )
                .unwrap();
        }

        tablets.push(tablet);
    }

    println!("  Tablets created (5 devices × 400K rows each)");

    // Now measure ONLY writing
    let temp_file = NamedTempFile::new().unwrap();
    let mut writer = TsFileWriter::new(temp_file.path()).unwrap();

    // Register schemas
    for device in &devices {
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new("temperature", TSDataType::Float, encoding, compression),
            )
            .unwrap();
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new("humidity", TSDataType::Float, encoding, compression),
            )
            .unwrap();
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new("pressure", TSDataType::Float, encoding, compression),
            )
            .unwrap();
    }

    let write_start = Instant::now();

    for tablet in tablets {
        writer.write_tablet(&tablet).unwrap();
    }

    let write_time = write_start.elapsed();
    println!("  Writing time: {:?}", write_time);

    let close_start = Instant::now();
    writer.close().unwrap();
    let close_time = close_start.elapsed();
    println!("  Close time: {:?}", close_time);

    let file_size = std::fs::metadata(temp_file.path()).unwrap().len();
    let total = write_start.elapsed();

    println!("  File size: {:.2} MB", file_size as f64 / 1_000_000.0);
    println!(
        "  Throughput: {:.2} MB/s",
        (file_size as f64 / 1_000_000.0) / total.as_secs_f64()
    );

    total
}

fn profile_full_pipeline(batch: &RecordBatch) -> std::time::Duration {
    println!("\n=== Profiling FULL Pipeline (Arrow → TsFile) ===");

    let temp_file = NamedTempFile::new().unwrap();
    let start = Instant::now();

    let mut converter = ArrowToTsFileConverter::builder(temp_file.path())
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();

    let init_time = start.elapsed();

    let write_start = Instant::now();
    converter.write_batch(batch).unwrap();
    let write_time = write_start.elapsed();

    let finish_start = Instant::now();
    converter.finish().unwrap();
    let finish_time = finish_start.elapsed();

    let total = start.elapsed();

    println!("  Init: {:?}", init_time);
    println!("  Write: {:?}", write_time);
    println!("  Finish: {:?}", finish_time);
    println!("  TOTAL: {:?}", total);

    total
}

fn main() {
    println!("Encoding vs Conversion Overhead Profiling");
    println!("==========================================\n");

    let num_rows = 2_000_000;
    println!(
        "Test dataset: {} rows (5 devices × 400K rows each)\n",
        num_rows
    );

    let batch = generate_test_batch(num_rows);

    // Test 1: Conversion only (no writing)
    let conversion_time = profile_arrow_conversion_only(&batch);

    // Test 2: Encoding only (different encodings)
    let encoding_chimp = profile_encoding_only(TSEncoding::Chimp128, CompressionType::Snappy);
    let encoding_plain = profile_encoding_only(TSEncoding::Plain, CompressionType::Snappy);
    let encoding_nocomp =
        profile_encoding_only(TSEncoding::Chimp128, CompressionType::Uncompressed);

    // Test 3: Full pipeline
    let full_time = profile_full_pipeline(&batch);

    println!("\n==========================================");
    println!("BREAKDOWN SUMMARY");
    println!("==========================================\n");

    println!(
        "Arrow → Tablet conversion: {:?} ({:.1}% of full pipeline)",
        conversion_time,
        100.0 * conversion_time.as_secs_f64() / full_time.as_secs_f64()
    );

    println!("\nEncoding overhead (Tablet → TsFile):");
    println!(
        "  Chimp128 + Snappy: {:?} ({:.1}% of full pipeline)",
        encoding_chimp,
        100.0 * encoding_chimp.as_secs_f64() / full_time.as_secs_f64()
    );
    println!(
        "  Plain + Snappy:    {:?} ({}x faster than Chimp128)",
        encoding_plain,
        encoding_chimp.as_secs_f64() / encoding_plain.as_secs_f64()
    );
    println!(
        "  Chimp128 + No compression: {:?} (compression overhead: {:?})",
        encoding_nocomp,
        encoding_chimp - encoding_nocomp
    );

    println!("\nFull pipeline: {:?}", full_time);

    println!("\n==========================================");
    println!("CONCLUSION:");
    println!("==========================================\n");

    let encoding_percentage = 100.0 * encoding_chimp.as_secs_f64() / full_time.as_secs_f64();
    let conversion_percentage = 100.0 * conversion_time.as_secs_f64() / full_time.as_secs_f64();

    if encoding_percentage > 60.0 {
        println!(
            "✅ BOTTLENECK CONFIRMED: Encoding is {:.1}% of total time",
            encoding_percentage
        );
        println!(
            "   Arrow conversion is only {:.1}% - already well optimized!",
            conversion_percentage
        );
        println!("\n   RECOMMENDATION: Focus optimization efforts on encoding layer:");
        println!("   - Profile Chimp128 implementation");
        println!("   - Consider SIMD optimizations");
        println!("   - Evaluate adaptive encoding (Plain for random data)");
    } else {
        println!(
            "⚠️  UNEXPECTED: Encoding is only {:.1}% of total time",
            encoding_percentage
        );
        println!("   Conversion is {:.1}%", conversion_percentage);
        println!("   Need deeper profiling to find real bottleneck.");
    }
}
