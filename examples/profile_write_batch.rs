use tsfile_rs::common::*;
use tsfile_rs::writer::TsFileWriter;
use std::time::Instant;

fn main() {
    let path = "/tmp/profile_write_batch.ts";
    let num_rows = 100_000;

    println!("=== Comparación: Row-by-Row vs Batch Writing ===\n");

    // Test 1: Row-by-row (current implementation)
    println!("1. ROW-BY-ROW (write_record):");
    let mut timings_row = Vec::new();
    for run in 0..5 {
        let start = Instant::now();
        write_row_by_row(path, num_rows);
        let elapsed = start.elapsed();
        timings_row.push(elapsed.as_millis());
        println!("  Run {}: {:?}", run + 1, elapsed);
        let _ = std::fs::remove_file(path);
    }
    let avg_row = timings_row.iter().sum::<u128>() / 5;
    println!("  Promedio: {}ms\n", avg_row);

    // Test 2: Batch writing with Tablets
    println!("2. BATCH WRITING (write_tablet):");
    let mut timings_batch = Vec::new();
    for run in 0..5 {
        let start = Instant::now();
        write_with_tablets(path, num_rows);
        let elapsed = start.elapsed();
        timings_batch.push(elapsed.as_millis());
        println!("  Run {}: {:?}", run + 1, elapsed);
        let _ = std::fs::remove_file(path);
    }
    let avg_batch = timings_batch.iter().sum::<u128>() / 5;
    println!("  Promedio: {}ms\n", avg_batch);

    // Comparison
    let speedup = avg_row as f64 / avg_batch as f64;
    let improvement = ((avg_row - avg_batch) as f64 / avg_row as f64) * 100.0;
    println!("=== Resultados ===");
    println!("Row-by-row:    {}ms", avg_row);
    println!("Batch writing: {}ms", avg_batch);
    println!("Speedup:       {:.2}x", speedup);
    println!("Improvement:   {:.1}%", improvement);
}

/// Current implementation: write_record() row-by-row
fn write_row_by_row(path: &str, total_rows: usize) {
    let _ = std::fs::remove_file(path);
    let mut writer = TsFileWriter::new(path).unwrap();

    // Register schemas
    for device_idx in 1..=5 {
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

    // Write row-by-row
    let rows_per_device = total_rows / 5;
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);
        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            let record = TsRecord::new(timestamp, &device_id)
                .with_value("temperature", TsValue::Float(25.0 + (i % 100) as f32 * 0.1))
                .with_value("pressure", TsValue::Float(1013.25 + (i % 50) as f32 * 0.5))
                .with_value("humidity", TsValue::Float(60.0 + (i % 40) as f32 * 0.25));
            writer.write_record(record).unwrap();
        }
    }

    writer.close().unwrap();
}

/// Optimized: write_tablet() batch writing
fn write_with_tablets(path: &str, total_rows: usize) {
    let _ = std::fs::remove_file(path);
    let mut writer = TsFileWriter::new(path).unwrap();

    let rows_per_device = total_rows / 5;

    // Write each device as a tablet (batch)
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);

        // Create schemas
        let schemas = vec![
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
        ];

        // Register schemas
        for schema in &schemas {
            writer
                .register_timeseries(&device_id, schema.clone())
                .unwrap();
        }

        // Create tablet
        let mut tablet = Tablet::new(
            &device_id,
            schemas,
            vec![ColumnCategory::Field; 3],
            rows_per_device,
        );

        // Add all rows to tablet in batch
        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            let temperature = TsValue::Float(25.0 + (i % 100) as f32 * 0.1);
            let pressure = TsValue::Float(1013.25 + (i % 50) as f32 * 0.5);
            let humidity = TsValue::Float(60.0 + (i % 40) as f32 * 0.25);

            tablet
                .add_row(timestamp, vec![Some(temperature), Some(pressure), Some(humidity)])
                .unwrap();
        }

        // Write entire tablet at once
        writer.write_tablet(&tablet).unwrap();
    }

    writer.close().unwrap();
}
