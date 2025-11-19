use tsfile::common::*;
use tsfile::writer::TsFileWriter;
use std::time::Instant;

fn main() {
    let path = "/tmp/profile_write.ts";
    let num_rows = 100_000;

    println!("=== Write Profiling ({}K rows) ===\n", num_rows / 1000);

    // Run múltiples veces para obtener media
    let mut timings = Vec::new();

    for run in 0..5 {
        let start = Instant::now();
        write_tsfile(path, num_rows);
        let elapsed = start.elapsed();
        timings.push(elapsed.as_millis());
        println!("Run {}: {:?}", run + 1, elapsed);

        // Limpiar para siguiente run
        let _ = std::fs::remove_file(path);
    }

    let avg = timings.iter().sum::<u128>() / timings.len() as u128;
    println!("\nPromedio: {}ms", avg);
}

fn write_tsfile(path: &str, total_rows: usize) {
    let _ = std::fs::remove_file(path);

    let mut writer = TsFileWriter::new(path).unwrap();

    // 5 devices × 3 measurements
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

    // Escribir rows_per_device rows por cada dispositivo
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
