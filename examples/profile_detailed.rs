use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

// Arrow imports
use arrow::array::{Array, Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

// TsFile imports
use tsfile::arrow::ArrowToTsFileConverter;
use tsfile::common::{ColumnCategory, CompressionType, MeasurementSchema, TSDataType, TSEncoding, Tablet, TsValue};
use tsfile::writer::TsFileWriter;

/// Generate test data
fn generate_test_data(num_rows: usize) -> RecordBatch {
    let mut timestamps = Vec::with_capacity(num_rows);
    let mut device_ids = Vec::with_capacity(num_rows);
    let mut temperatures = Vec::with_capacity(num_rows);
    let mut humidity = Vec::with_capacity(num_rows);
    let mut pressure = Vec::with_capacity(num_rows);

    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64;

    for i in 0..num_rows {
        timestamps.push(base_time + (i as i64 * 1000));
        device_ids.push(devices[i % devices.len()]);

        let device_offset = (i % devices.len()) as f32 * 5.0;
        temperatures.push(20.0 + device_offset + (i as f32 * 0.001) % 10.0);
        humidity.push(50.0 + device_offset + (i as f32 * 0.002) % 20.0);
        pressure.push(1013.0 + device_offset + (i as f32 * 0.0005) % 50.0);
    }

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

    let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps));
    let device_array = Arc::new(StringArray::from(device_ids));
    let temp_array = Arc::new(Float32Array::from(temperatures));
    let humidity_array = Arc::new(Float32Array::from(humidity));
    let pressure_array = Arc::new(Float32Array::from(pressure));

    RecordBatch::try_new(
        schema,
        vec![
            timestamp_array,
            device_array,
            temp_array,
            humidity_array,
            pressure_array,
        ],
    )
    .unwrap()
}

/// Manual simulation of write_batch to profile each step
fn profile_write_batch_detailed(batch: &RecordBatch) {
    println!("\n=== DETAILED PROFILING OF write_batch() ===\n");

    let temp_file = NamedTempFile::new().unwrap();
    let mut writer = TsFileWriter::new(temp_file.path()).unwrap();

    let start_total = Instant::now();

    // STEP 1: Extract arrays
    let start_step = Instant::now();
    let device_array = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let timestamp_array = batch
        .column(0)
        .as_any()
        .downcast_ref::<TimestampMillisecondArray>()
        .unwrap();

    let measurement_cols: Vec<(String, Arc<dyn Array>, DataType)> = vec![
        ("temperature".to_string(), batch.column(2).clone(), DataType::Float32),
        ("humidity".to_string(), batch.column(3).clone(), DataType::Float32),
        ("pressure".to_string(), batch.column(4).clone(), DataType::Float32),
    ];
    let step1_time = start_step.elapsed();

    // STEP 2: Group row indices by device
    let start_step = Instant::now();
    let mut device_indices: HashMap<String, Vec<usize>> = HashMap::new();
    let num_rows = batch.num_rows();
    for row_idx in 0..num_rows {
        if !device_array.is_null(row_idx) {
            let device_id = device_array.value(row_idx).to_string();
            device_indices.entry(device_id).or_insert_with(Vec::new).push(row_idx);
        }
    }
    let step2_time = start_step.elapsed();

    // STEP 3: Process each device
    let start_step = Instant::now();
    let mut step3_breakdown = Vec::new();

    for (device_id, indices) in device_indices {
        let start_device = Instant::now();

        // Register schemas
        for (col_name, _, _) in &measurement_cols {
            writer.register_timeseries(
                &device_id,
                MeasurementSchema::new(
                    col_name.clone(),
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            ).unwrap();
        }
        let register_time = start_device.elapsed();

        // Build schemas and tablet
        let start_tablet_creation = Instant::now();
        let schemas = measurement_cols.iter().map(|(col_name, _, _)| {
            MeasurementSchema::new(
                col_name.clone(),
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            )
        }).collect::<Vec<_>>();

        let column_categories = vec![ColumnCategory::Field; schemas.len()];
        let mut tablet = Tablet::new(&device_id, schemas, column_categories, indices.len());
        let tablet_creation_time = start_tablet_creation.elapsed();

        // Extract timestamps
        let start_extract_ts = Instant::now();
        let device_timestamps: Vec<i64> = indices.iter().map(|&idx| timestamp_array.value(idx)).collect();
        let extract_ts_time = start_extract_ts.elapsed();

        // Extract column data (CRITICAL PATH)
        let start_extract_cols = Instant::now();
        let mut column_values: Vec<Vec<Option<TsValue>>> = Vec::with_capacity(measurement_cols.len());

        for (_, column, _) in &measurement_cols {
            let arr = column.as_any().downcast_ref::<Float32Array>().unwrap();
            let mut col_data = Vec::with_capacity(indices.len());
            for &idx in &indices {
                col_data.push(if arr.is_null(idx) {
                    None
                } else {
                    Some(TsValue::Float(arr.value(idx)))
                });
            }
            column_values.push(col_data);
        }
        let extract_cols_time = start_extract_cols.elapsed();

        // Bulk add rows
        let start_bulk_add = Instant::now();
        tablet.add_rows_bulk(&device_timestamps, column_values).unwrap();
        let bulk_add_time = start_bulk_add.elapsed();

        // Write tablet
        let start_write = Instant::now();
        writer.write_tablet(&tablet).unwrap();
        let write_time = start_write.elapsed();

        let total_device_time = start_device.elapsed();

        step3_breakdown.push((
            device_id.clone(),
            indices.len(),
            register_time,
            tablet_creation_time,
            extract_ts_time,
            extract_cols_time,
            bulk_add_time,
            write_time,
            total_device_time,
        ));
    }

    let step3_time = start_step.elapsed();

    let total_time = start_total.elapsed();

    // Print results
    println!("STEP 1 (Extract arrays):        {:>8.2} ms ({:>5.1}%)",
        step1_time.as_secs_f64() * 1000.0,
        step1_time.as_secs_f64() / total_time.as_secs_f64() * 100.0);

    println!("STEP 2 (Group by device):       {:>8.2} ms ({:>5.1}%)",
        step2_time.as_secs_f64() * 1000.0,
        step2_time.as_secs_f64() / total_time.as_secs_f64() * 100.0);

    println!("STEP 3 (Process devices):       {:>8.2} ms ({:>5.1}%)",
        step3_time.as_secs_f64() * 1000.0,
        step3_time.as_secs_f64() / total_time.as_secs_f64() * 100.0);

    println!("\n--- STEP 3 Breakdown (per device) ---");
    for (device_id, num_rows, reg, tablet_c, ext_ts, ext_cols, bulk, write, total_dev) in &step3_breakdown {
        println!("  {} ({} rows):", device_id, num_rows);
        println!("    Register:         {:>6.2} ms ({:>5.1}%)", reg.as_secs_f64() * 1000.0, reg.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Tablet creation:  {:>6.2} ms ({:>5.1}%)", tablet_c.as_secs_f64() * 1000.0, tablet_c.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Extract TS:       {:>6.2} ms ({:>5.1}%)", ext_ts.as_secs_f64() * 1000.0, ext_ts.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Extract cols:     {:>6.2} ms ({:>5.1}%)", ext_cols.as_secs_f64() * 1000.0, ext_cols.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Bulk add:         {:>6.2} ms ({:>5.1}%)", bulk.as_secs_f64() * 1000.0, bulk.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Write tablet:     {:>6.2} ms ({:>5.1}%)", write.as_secs_f64() * 1000.0, write.as_secs_f64() / total_dev.as_secs_f64() * 100.0);
        println!("    Total device:     {:>6.2} ms", total_dev.as_secs_f64() * 1000.0);
    }

    println!("\n─────────────────────────────────────");
    println!("Total time:                  {:>8.2} ms", total_time.as_secs_f64() * 1000.0);

    // Calculate aggregates for step 3
    let total_register = step3_breakdown.iter().map(|(_, _, r, _, _, _, _, _, _)| r.as_secs_f64()).sum::<f64>() * 1000.0;
    let total_extract_cols = step3_breakdown.iter().map(|(_, _, _, _, _, e, _, _, _)| e.as_secs_f64()).sum::<f64>() * 1000.0;
    let total_bulk_add = step3_breakdown.iter().map(|(_, _, _, _, _, _, b, _, _)| b.as_secs_f64()).sum::<f64>() * 1000.0;
    let total_write = step3_breakdown.iter().map(|(_, _, _, _, _, _, _, w, _)| w.as_secs_f64()).sum::<f64>() * 1000.0;

    println!("\n=== HOT PATHS ===");
    println!("Extract columns:  {:>8.2} ms ({:>5.1}% of total)", total_extract_cols, total_extract_cols / (total_time.as_secs_f64() * 1000.0) * 100.0);
    println!("Bulk add:         {:>8.2} ms ({:>5.1}% of total)", total_bulk_add, total_bulk_add / (total_time.as_secs_f64() * 1000.0) * 100.0);
    println!("Write tablet:     {:>8.2} ms ({:>5.1}% of total)", total_write, total_write / (total_time.as_secs_f64() * 1000.0) * 100.0);
    println!("Register schemas: {:>8.2} ms ({:>5.1}% of total)", total_register, total_register / (total_time.as_secs_f64() * 1000.0) * 100.0);
}

fn main() {
    println!("Generating 100,000 rows of test data...");
    let batch = generate_test_data(100_000);

    profile_write_batch_detailed(&batch);
}
